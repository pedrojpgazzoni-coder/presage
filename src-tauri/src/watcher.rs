//! Loop que observa o cliente e empurra estado para o front.
//!
//! Estratégia: polling adaptativo. 350ms durante o champ select (uma fase dura
//! ~30s, então isso é tempo real para todo efeito prático), 1,5s em outras
//! fases, 3s quando o cliente não está aberto.
//!
//! TODO(fase 1): trocar por WebSocket (`wss://127.0.0.1:<porta>/`, mesma Basic
//! Auth, subscrever `OnJsonApiEvent_lol-champ-select_v1_session`). O polling
//! está aqui porque é mais simples de acertar e o custo é irrelevante nesta
//! escala; o WS vira ganho real quando começarmos a reagir a hover de aliado.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use presage_core::engine::champion::ChampionIndex;
use presage_core::engine::coach::{Recording, Report};
use presage_core::engine::dataset::Dataset;
use presage_core::engine::items::ItemIndex;
use presage_core::engine::runes::RuneIndex;
use presage_core::engine::stats::{NoStats, StatsStore};
use presage_core::engine::priority;
use presage_core::engine::timeline::{self, Timings};
use presage_core::live::to_live_game;
use presage_core::pool::Pool;
use presage_core::prefs::OverlayPrefs;

use crate::lcu::models::{to_draft_state, ConnStatus, DraftState};
use crate::lcu::{credentials, LcuClient};
use crate::live::{LiveClient, LiveSnapshot};
use crate::recorder;

pub const EVENT_STATUS: &str = "presage://status";
pub const EVENT_DRAFT: &str = "presage://draft";
pub const EVENT_LIVE: &str = "presage://live";
pub const EVENT_REVIEW: &str = "presage://review";
/// O jogador mexeu no que o overlay mostra. O overlay escuta e se redesenha
/// na hora, sem precisar fechar nada.
pub const EVENT_PREFS: &str = "presage://prefs";
/// Nick, rank e histórico chegaram do cliente.
pub const EVENT_PROFILE: &str = "presage://profile";

/// Quantas partidas o histórico traz.
const HISTORICO: usize = 20;

const POLL_CHAMP_SELECT: Duration = Duration::from_millis(350);
const POLL_LIVE: Duration = Duration::from_millis(1000);
const POLL_IDLE: Duration = Duration::from_millis(1500);
const POLL_DISCONNECTED: Duration = Duration::from_millis(3000);

/// Último estado conhecido, para o front poder pedir um snapshot ao montar em
/// vez de esperar o próximo tick.
/// Quem está logado no cliente. Sai do próprio LCU, então o usuário não
/// precisa digitar o Riot ID em lugar nenhum.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub puuid: String,
    pub riot_id: String,
    pub region: String,
}

pub struct Shared {
    pub status: Mutex<Option<ConnStatus>>,
    pub draft: Mutex<Option<DraftState>>,
    pub champions: RwLock<ChampionIndex>,
    pub items: RwLock<ItemIndex>,
    pub runes: RwLock<RuneIndex>,
    pub live: Mutex<Option<LiveSnapshot>>,
    pub stats: RwLock<Arc<dyn StatsStore>>,
    pub identity: Mutex<Option<Identity>>,
    pub pool: RwLock<Pool>,
    /// O que o jogador quer ver no overlay.
    pub overlay_prefs: RwLock<OverlayPrefs>,
    /// A partida sendo gravada agora, quadro a quadro.
    pub recording: Mutex<Recording>,
    /// A leitura da última partida terminada.
    pub last_review: Mutex<Option<Report>>,
    /// Base de partidas agregada. `None` até existir um pipeline que a produza
    /// — e `None` é o estado honesto, não um defeito.
    pub dataset: RwLock<Option<Dataset>>,
    /// Nick, rank e últimas partidas, lidos do cliente local.
    pub profile: Mutex<Option<crate::lcu::history::Profile>>,
    /// A primeira execução já foi respondida.
    pub onboarding: Mutex<bool>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(None),
            draft: Mutex::new(None),
            champions: RwLock::new(ChampionIndex::default()),
            items: RwLock::new(ItemIndex::default()),
            runes: RwLock::new(RuneIndex::default()),
            live: Mutex::new(None),
            stats: RwLock::new(Arc::new(NoStats)),
            identity: Mutex::new(None),
            pool: RwLock::new(Pool::default()),
            overlay_prefs: RwLock::new(OverlayPrefs::default()),
            recording: Mutex::new(Recording::default()),
            last_review: Mutex::new(None),
            dataset: RwLock::new(None),
            profile: Mutex::new(None),
            onboarding: Mutex::new(false),
        }
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

/// Fecha a gravação da partida que acabou e publica a leitura.
///
/// Sai cedo quando não há nada gravado, para o loop ocioso não tocar em disco
/// a cada volta e meia.
fn close_recording(app: &AppHandle, shared: &Shared) {
    if shared.recording.lock().unwrap().frames.is_empty() {
        return;
    }

    let items = shared.items.read().unwrap().clone();
    let report = recorder::finish(&mut shared.recording.lock().unwrap(), &items);

    if let Some(report) = report {
        tracing::info!(
            bons = report.good.len(),
            ruins = report.bad.len(),
            "leitura da partida pronta"
        );
        let _ = app.emit(EVENT_REVIEW, &report);
        *shared.last_review.lock().unwrap() = Some(report);
    }
}

fn clear_draft(app: &AppHandle, shared: &Shared) {
    if shared.draft.lock().unwrap().take().is_some() {
        let _ = app.emit::<Option<DraftState>>(EVENT_DRAFT, None);
    }
}

pub async fn run(app: AppHandle, shared: Arc<Shared>) {
    let mut client: Option<LcuClient> = None;
    let mut summoner: Option<String> = None;

    let live_client = match LiveClient::new() {
        Ok(client) => client,
        Err(err) => {
            tracing::error!(%err, "não consegui montar o cliente da Live Client Data API");
            return;
        }
    };

    loop {
        // Credenciais mudam a cada reinício do cliente, então reconectar é o
        // caminho normal, não o excepcional.
        if client.is_none() {
            match credentials::discover().and_then(|c| LcuClient::new(&c)) {
                Ok(c) => {
                    if let Ok(me) = c.current_summoner().await {
                        let riot_id = if me.tag_line.is_empty() {
                            me.game_name.clone()
                        } else {
                            format!("{}#{}", me.game_name, me.tag_line)
                        };
                        summoner = Some(riot_id.clone());

                        if !me.puuid.is_empty() {
                            let region = c.region().await.unwrap_or_else(|_| "BR".into());
                            *shared.identity.lock().unwrap() = Some(Identity {
                                puuid: me.puuid,
                                riot_id,
                                region,
                            });
                        }
                    }

                    // Nick, rank e histórico, do próprio cliente. É o que a
                    // tela inicial mostra, e não depende de chave nenhuma.
                    match crate::lcu::history::profile(&c, HISTORICO).await {
                        Ok(perfil) => {
                            tracing::info!(
                                quem = %perfil.riot_id,
                                partidas = perfil.matches.len(),
                                "perfil carregado do cliente"
                            );
                            let _ = app.emit(EVENT_PROFILE, &perfil);
                            *shared.profile.lock().unwrap() = Some(perfil);
                        }
                        Err(err) => tracing::warn!(%err, "não consegui ler o perfil"),
                    }

                    client = Some(c);
                }
                Err(err) => {
                    publish_status(
                        &app,
                        &shared,
                        ConnStatus {
                            connected: false,
                            phase: "Disconnected".into(),
                            summoner: None,
                            detail: Some(err.to_string()),
                        },
                    );
                    tokio::time::sleep(POLL_DISCONNECTED).await;
                    continue;
                }
            }
        }

        let lcu = client.as_ref().expect("cliente garantido acima");

        let phase = match lcu.gameflow_phase().await {
            Ok(phase) => phase,
            Err(err) => {
                // Cliente fechou ou as credenciais expiraram: derruba e
                // redescobre na próxima volta.
                tracing::debug!(%err, "perdemos o cliente, redescobrindo");
                client = None;
                summoner = None;
                // O perfil fica: nome e histórico não deixam de ser verdade
                // porque o cliente fechou, e apagá-los pisca a tela à toa.
                *shared.draft.lock().unwrap() = None;
                continue;
            }
        };

        publish_status(
            &app,
            &shared,
            ConnStatus {
                connected: true,
                phase: phase.clone(),
                summoner: summoner.clone(),
                detail: None,
            },
        );

        if phase == "ChampSelect" {
            match lcu.champ_select_session().await {
                Ok(session) => {
                    let state = to_draft_state(&session);
                    let mut cached = shared.draft.lock().unwrap();
                    // Só emite quando algo mudou de fato. O timer sozinho muda a
                    // cada tick, então ele fica fora da comparação: o front
                    // interpola o relógio localmente.
                    let changed = cached
                        .as_ref()
                        .map(|prev| !same_draft(prev, &state))
                        .unwrap_or(true);
                    if changed {
                        let _ = app.emit(EVENT_DRAFT, &state);
                    }
                    *cached = Some(state);
                }
                Err(err) => tracing::debug!(%err, "falha ao ler a sessão de champ select"),
            }
            tokio::time::sleep(POLL_CHAMP_SELECT).await;
        } else if phase == "InProgress" {
            clear_draft(&app, &shared);

            match live_client.snapshot().await {
                Ok(raw) => {
                    let snapshot = {
                        let champions = shared.champions.read().unwrap();
                        let items = shared.items.read().unwrap();

                        let game =
                            to_live_game(&raw.players, &raw.active, raw.game_time, &champions);

                        // Guarda o quadro antes de montar o resto: o coach
                        // pós-partida só tem o que foi gravado enquanto o jogo
                        // acontecia.
                        recorder::observe(
                            &mut shared.recording.lock().unwrap(),
                            &game,
                            &raw.events,
                        );

                        let timeline =
                            timeline::build(&raw.events, &game, &Timings::default());
                        let priority = priority::report(&game, &timeline, &champions, &items);

                        LiveSnapshot {
                            game,
                            timeline,
                            priority,
                        }
                    };

                    let changed = shared
                        .live
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|previous| previous != &snapshot)
                        .unwrap_or(true);

                    if changed {
                        let _ = app.emit(EVENT_LIVE, &snapshot);
                    }
                    *shared.live.lock().unwrap() = Some(snapshot);
                }
                // Carregando a partida, ou já acabou. Nenhum dos dois é erro.
                Err(err) => tracing::trace!(%err, "Live Client ainda não responde"),
            }

            tokio::time::sleep(POLL_LIVE).await;
        } else {
            clear_draft(&app, &shared);
            if shared.live.lock().unwrap().take().is_some() {
                let _ = app.emit::<Option<LiveSnapshot>>(EVENT_LIVE, None);
            }

            // Saímos de `InProgress`: a partida acabou. Fecha a gravação e
            // publica a leitura. `finish` limpa a gravação mesmo quando não
            // rende relatório, então isto não dispara duas vezes.
            close_recording(&app, &shared);

            tokio::time::sleep(POLL_IDLE).await;
        }
    }
}

/// Igualdade ignorando o relógio.
fn same_draft(a: &DraftState, b: &DraftState) -> bool {
    a.phase == b.phase
        && a.my_team == b.my_team
        && a.their_team == b.their_team
        && a.my_bans == b.my_bans
        && a.their_bans == b.their_bans
        && a.current_action == b.current_action
        && a.is_local_turn == b.is_local_turn
}

fn publish_status(app: &AppHandle, shared: &Shared, status: ConnStatus) {
    let mut cached = shared.status.lock().unwrap();
    if cached.as_ref() != Some(&status) {
        let _ = app.emit(EVENT_STATUS, &status);
        *cached = Some(status);
    }
}
