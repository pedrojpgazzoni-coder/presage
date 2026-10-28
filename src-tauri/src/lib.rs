pub mod analysis;
pub mod ddragon;
pub mod lcu;
pub mod live;
pub mod overlay;
pub mod profile;
pub mod recorder;
pub mod riot;
pub mod storage;
pub mod watcher;

use serde::Serialize;
use std::sync::Arc;
use tauri::{Emitter, Manager};

use analysis::LaneReport;
use lcu::models::{ConnStatus, DraftState};
use presage_core::engine::build::{self, BuildContext, ItemSuggestion};
use presage_core::engine::champion::Role;
use presage_core::engine::coach::Report;
use presage_core::engine::dataset::{Dataset, Layered};
use presage_core::engine::gold::{self, GoldReport};
use presage_core::engine::lanes;
use presage_core::engine::poolgap;
use presage_core::engine::runes;
use presage_core::engine::minimax;
use presage_core::engine::stats::PersonalStats;
use presage_core::engine::{self, Scored};
use live::LiveSnapshot;
use presage_core::pool::Pool;
use presage_core::prefs::OverlayPrefs;
use watcher::Shared;

const EVENT_ENGINE: &str = "presage://engine";
const POOL_CACHE: &str = "pool.json";
const PREFS_CACHE: &str = "overlay.json";
const ONBOARDING_CACHE: &str = "primeira-vez.json";
/// Base de partidas agregada. Não vem no instalador; quando existir, é aqui
/// que ela mora.
const DATASET_CACHE: &str = "base-de-partidas.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineStatus {
    ready: bool,
    version: String,
    champions: usize,
    has_global: bool,
    has_personal: bool,
    personal_games: u32,
    account: Option<String>,
    /// Patch de onde a base de partidas saiu, quando existe. A interface mostra
    /// para ninguém confiar numa base de dois patches atrás sem saber.
    global_patch: Option<String>,
    global_matches: u64,
}

fn engine_status_of(shared: &Shared) -> EngineStatus {
    let champions = shared.champions.read().unwrap();
    let stats = shared.stats.read().unwrap();
    let dataset = shared.dataset.read().unwrap();

    EngineStatus {
        ready: !champions.is_empty(),
        version: champions.version.clone(),
        champions: champions.by_id.len(),
        has_global: stats.has_global(),
        has_personal: stats.has_personal(),
        personal_games: stats.personal_games(),
        account: shared
            .identity
            .lock()
            .unwrap()
            .as_ref()
            .map(|id| id.riot_id.clone()),
        global_patch: dataset.as_ref().map(|d| d.patch.clone()),
        global_matches: dataset.as_ref().map_or(0, |d| d.matches),
    }
}

/// Reconstrói a loja de estatísticas a partir das duas camadas.
///
/// As duas são independentes: a base agregada responde winrate, matchup e
/// sinergia; o histórico da conta responde só o componente pessoal. Trocar uma
/// não pode apagar a outra, e é por isso que a montagem fica num lugar só.
fn rebuild_stats(shared: &Shared, personal: PersonalStats) {
    let global = shared.dataset.read().unwrap().clone();
    *shared.stats.write().unwrap() = Arc::new(Layered { global, personal });
}

#[tauri::command]
fn get_snapshot(shared: tauri::State<'_, Arc<Shared>>) -> (Option<ConnStatus>, Option<DraftState>) {
    (
        shared.status.lock().unwrap().clone(),
        shared.draft.lock().unwrap().clone(),
    )
}

#[tauri::command]
fn engine_status(shared: tauri::State<'_, Arc<Shared>>) -> EngineStatus {
    engine_status_of(&shared)
}

#[tauri::command]
fn rank_draft(shared: tauri::State<'_, Arc<Shared>>) -> Vec<Scored> {
    let draft = shared.draft.lock().unwrap().clone();
    let Some(draft) = draft else {
        return Vec::new();
    };

    let role = Role::from_lcu(&draft.local_position);
    let ids = shared.pool.read().unwrap().ids_for_role(role);

    let champions = shared.champions.read().unwrap();
    let stats = shared.stats.read().unwrap();
    engine::rank(&draft, &champions, &**stats, &ids)
}

/// Ranking com uma camada de resposta adversária.
#[tauri::command]
fn minimax_draft(shared: tauri::State<'_, Arc<Shared>>) -> Vec<minimax::MinimaxPick> {
    let draft = shared.draft.lock().unwrap().clone();
    let Some(draft) = draft else {
        return Vec::new();
    };

    let role = Role::from_lcu(&draft.local_position);
    let ids = shared.pool.read().unwrap().ids_for_role(role);
    let champions = shared.champions.read().unwrap();

    minimax::minimax(&draft, &champions, &ids, 12, 4)
}

#[tauri::command]
fn ban_suggestions(shared: tauri::State<'_, Arc<Shared>>) -> Vec<minimax::BanSuggestion> {
    let draft = shared.draft.lock().unwrap().clone();
    let Some(draft) = draft else {
        return Vec::new();
    };
    let champions = shared.champions.read().unwrap();
    minimax::suggest_bans(&draft, &champions, 10)
}

#[tauri::command]
fn get_live(shared: tauri::State<'_, Arc<Shared>>) -> Option<LiveSnapshot> {
    shared.live.lock().unwrap().clone()
}

/// Build adaptativa para a partida em andamento.
#[tauri::command]
fn build_suggestions(
    limit: Option<usize>,
    shared: tauri::State<'_, Arc<Shared>>,
) -> Option<(BuildContext, Vec<ItemSuggestion>)> {
    let snapshot = shared.live.lock().unwrap().clone()?;
    let game = snapshot.game;
    let me = game.me()?.as_combatant();
    let enemies: Vec<_> = game
        .enemies()
        .into_iter()
        .map(|enemy| enemy.as_combatant())
        .collect();

    let champions = shared.champions.read().unwrap();
    let items = shared.items.read().unwrap();

    Some(build::suggest(
        &me,
        &enemies,
        &champions,
        &items,
        limit.unwrap_or(8),
    ))
}

/// Chance de vitória por rota e em cada escaramuça.
///
/// Serve o draft e a partida com a mesma chamada: durante o jogo os níveis e
/// itens são reais, antes dele é projeção em igualdade.
#[tauri::command]
fn lane_analysis(shared: tauri::State<'_, Arc<Shared>>) -> Option<LaneReport> {
    let live = shared.live.lock().unwrap().clone();
    let draft = shared.draft.lock().unwrap().clone();
    let champions = shared.champions.read().unwrap();
    let items = shared.items.read().unwrap();
    let stats = shared.stats.read().unwrap();

    analysis::report(
        live.as_ref().map(|snapshot| &snapshot.game),
        draft.as_ref(),
        &champions,
        &items,
        &**stats,
    )
}

/// A escaramuça que importa agora — uma só, para o overlay.
///
/// No painel cabem as cinco; por cima do jogo cabe uma, e ela tem de ser a
/// certa. Quem escolhe é `lanes::focus`, pelo objetivo mais próximo e pela
/// rota do jogador.
#[tauri::command]
fn skirmish_focus(shared: tauri::State<'_, Arc<Shared>>) -> Option<lanes::Focus> {
    let snapshot = shared.live.lock().unwrap().clone()?;

    let champions = shared.champions.read().unwrap();
    let items = shared.items.read().unwrap();
    let stats = shared.stats.read().unwrap();

    let report = analysis::report(
        Some(&snapshot.game),
        None,
        &champions,
        &items,
        &**stats,
    )?;

    let role = Role::from_lcu(&snapshot.game.me()?.position);
    lanes::focus(&report.skirmishes, &snapshot.timeline.upcoming, role)
}

/// Ouro dos dez. Investido é exato; ganho é estimativa, e o relatório diz qual
/// é qual.
#[tauri::command]
fn gold_report(shared: tauri::State<'_, Arc<Shared>>) -> Option<GoldReport> {
    let snapshot = shared.live.lock().unwrap().clone()?;
    Some(gold::report(&snapshot.game))
}

/// A leitura da última partida terminada.
#[tauri::command]
fn last_review(shared: tauri::State<'_, Arc<Shared>>) -> Option<Report> {
    shared.last_review.lock().unwrap().clone()
}

/// O que falta na pool desta rota, e quem preenche.
///
/// É o terceiro passo do Construtor: depois dos core e dos situacionais, o
/// último lugar é escolhido por cobertura, não por gosto.
#[tauri::command]
fn pool_suggestions(
    role: Option<String>,
    limit: Option<usize>,
    shared: tauri::State<'_, Arc<Shared>>,
) -> (Vec<poolgap::Gap>, Vec<poolgap::Candidate>) {
    let role = role.map(|r| Role::from_lcu(&r)).unwrap_or(Role::Unknown);
    let ids = shared.pool.read().unwrap().ids_for_role(role);
    let champions = shared.champions.read().unwrap();

    (
        poolgap::gaps(&ids, &champions),
        poolgap::suggest(&ids, &champions, limit.unwrap_or(8)),
    )
}

/// Runa principal para o confronto da sua rota.
///
/// Sai de regras mecânicas sobre o matchup, não de winrate — `from_data` no
/// resultado é falso enquanto a base de partidas não existir, e é por ele que
/// a interface decide o que escrever.
#[tauri::command]
fn rune_plan(shared: tauri::State<'_, Arc<Shared>>) -> Option<runes::RunePlan> {
    let live = shared.live.lock().unwrap().clone();
    let draft = shared.draft.lock().unwrap().clone();

    let champions = shared.champions.read().unwrap();
    let items = shared.items.read().unwrap();
    let stats = shared.stats.read().unwrap();
    let rune_index = shared.runes.read().unwrap();

    let report = analysis::report(
        live.as_ref().map(|s| &s.game),
        draft.as_ref(),
        &champions,
        &items,
        &**stats,
    )?;

    // A rota do jogador é a que interessa; as outras quatro não mudam a runa
    // dele.
    let minha = report.lanes.iter().find(|lane| {
        live.as_ref()
            .and_then(|s| s.game.me())
            .map(|me| Role::from_lcu(&me.position) == lane.role)
            .unwrap_or_else(|| {
                draft
                    .as_ref()
                    .map(|d| Role::from_lcu(&d.local_position) == lane.role)
                    .unwrap_or(false)
            })
    })?;

    runes::plan(minha.ally, minha.enemy, minha.role, &champions, &rune_index)
}

#[tauri::command]
fn get_pool(shared: tauri::State<'_, Arc<Shared>>) -> Pool {
    shared.pool.read().unwrap().clone()
}

#[tauri::command]
fn set_pool(pool: Pool, shared: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    storage::write_json(POOL_CACHE, &pool).map_err(|err| err.to_string())?;
    *shared.pool.write().unwrap() = pool;
    Ok(())
}

/// A primeira execução já foi respondida?
///
/// Guardado em arquivo, e não deduzido de "a pool tem campeões": quem esvazia
/// a pool para refazer não quer a tela de boas-vindas de volta.
#[tauri::command]
fn onboarding_done(shared: tauri::State<'_, Arc<Shared>>) -> bool {
    *shared.onboarding.lock().unwrap()
}

#[tauri::command]
fn finish_onboarding(shared: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    storage::write_json(ONBOARDING_CACHE, &true).map_err(|err| err.to_string())?;
    *shared.onboarding.lock().unwrap() = true;
    Ok(())
}

/// Nick, rank e últimas partidas, lidos do cliente local.
///
/// Nada aqui passa pela Riot API: é o próprio cliente do jogador servindo os
/// dados dele em `127.0.0.1`. Por isso funciona hoje, sem chave de produção.
#[tauri::command]
fn lcu_profile(shared: tauri::State<'_, Arc<Shared>>) -> Option<lcu::history::Profile> {
    shared.profile.lock().unwrap().clone()
}

#[tauri::command]
fn get_overlay_prefs(shared: tauri::State<'_, Arc<Shared>>) -> OverlayPrefs {
    shared.overlay_prefs.read().unwrap().clone()
}

/// Troca o que o overlay mostra e avisa a janela na hora.
///
/// O evento é o que faz a mudança ser imediata: sem ele o jogador mexeria no
/// painel durante o draft e só veria o efeito na partida seguinte.
#[tauri::command]
fn set_overlay_prefs(
    prefs: OverlayPrefs,
    app: tauri::AppHandle,
    shared: tauri::State<'_, Arc<Shared>>,
) -> Result<(), String> {
    storage::write_json(PREFS_CACHE, &prefs).map_err(|err| err.to_string())?;
    *shared.overlay_prefs.write().unwrap() = prefs.clone();
    let _ = app.emit(watcher::EVENT_PREFS, &prefs);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "presage_lib=debug,warn".into()),
        )
        .init();

    // O atualizador não pode impedir o app de abrir.
    //
    // O Tauri recusa endpoint que não seja https, e a recusa acontece na
    // inicialização do plugin — ou seja, derruba o processo inteiro. Numa
    // ferramenta que alguém instalou para usar, isso é o pior resultado
    // possível: um erro de digitação no endereço deixa todo mundo sem app, e
    // como o atualizador morreu junto não há como consertar remotamente.
    //
    // Então: tenta com ele, e se a configuração dele estiver errada, sobe sem.
    // Fica sem atualização automática, que é ruim — mas abre, que é o que
    // importa.
    if let Err(err) = montar(true).run(tauri::generate_context!()) {
        let texto = err.to_string();
        if !erro_do_atualizador(&texto) {
            panic!("falha ao iniciar o Presage: {texto}");
        }
        tracing::error!(
            erro = %texto,
            "configuração do atualizador recusada; abrindo sem atualização automática"
        );
        montar(false)
            .run(tauri::generate_context!())
            .expect("falha ao iniciar o Presage");
    }
}

/// A falha veio do atualizador?
///
/// É um casamento de texto, e por isso é a parte mais frágil do conserto: se a
/// mensagem do Tauri mudar, o app volta a morrer na inicialização. Daí o teste
/// abaixo prender a mensagem real, copiada de um crash de verdade.
///
/// Larga de propósito do lado de reconhecer: ficar sem atualização automática
/// é ruim, o app não abrir é pior. Mas não tão larga a ponto de engolir
/// qualquer erro — um defeito nosso tem de aparecer alto.
fn erro_do_atualizador(texto: &str) -> bool {
    let baixo = texto.to_lowercase();
    baixo.contains("updater") || baixo.contains("plugins.updater")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copiada de um crash de verdade: endpoint `http://` numa build release.
    const MENSAGEM_REAL: &str = "PluginInitialization(\"updater\", \"Error deserializing \
        'plugins.updater' within your Tauri configuration: The configured updater endpoint \
        must use a secure protocol like `https`.\")";

    #[test]
    fn reconhece_a_mensagem_real_do_tauri() {
        assert!(
            erro_do_atualizador(MENSAGEM_REAL),
            "sem isto o app volta a morrer na inicialização"
        );
    }

    #[test]
    fn reconhece_outras_falhas_do_mesmo_plugin() {
        for texto in [
            "PluginInitialization(\"updater\", \"missing field `pubkey`\")",
            "failed to initialize plugin updater",
            "Error deserializing 'plugins.updater'",
        ] {
            assert!(erro_do_atualizador(texto), "{texto}");
        }
    }

    #[test]
    fn nao_engole_erro_que_nao_e_do_atualizador() {
        // O risco do outro lado: esconder um defeito nosso atrás de uma
        // segunda tentativa silenciosa. Estes têm de continuar derrubando o
        // app, alto e claro.
        for texto in [
            "PluginInitialization(\"global-shortcut\", \"atalho em uso\")",
            "webview2 runtime not found",
            "failed to create window",
            "",
        ] {
            assert!(!erro_do_atualizador(texto), "{texto}");
        }
    }
}

fn montar(com_atualizador: bool) -> tauri::Builder<tauri::Wry> {
    let mut builder = tauri::Builder::default();

    if com_atualizador {
        // Os dois andam juntos: um baixa e instala, o outro reinicia depois.
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }

    builder
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        overlay::on_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let shared = Arc::new(Shared::new());
            if let Some(saved) = storage::read_json::<Pool>(POOL_CACHE) {
                tracing::info!(campeoes = saved.entries.len(), "pool carregado do disco");
                *shared.pool.write().unwrap() = saved;
            }

            // Base de partidas, se alguém já a colocou no lugar. `is_usable`
            // recusa arquivo truncado: melhor ficar sem base do que recomendar
            // a partir de meia dúzia de linhas.
            match storage::read_json::<Dataset>(DATASET_CACHE) {
                Some(dataset) if dataset.is_usable() => {
                    tracing::info!(
                        patch = %dataset.patch,
                        partidas = dataset.matches,
                        "base de partidas carregada"
                    );
                    *shared.dataset.write().unwrap() = Some(dataset);
                }
                Some(_) => tracing::warn!("base de partidas incompleta, ignorada"),
                None => tracing::info!("sem base de partidas: matchup e sinergia ficam em zero"),
            }
            rebuild_stats(&shared, PersonalStats::default());

            if let Some(saved) = storage::read_json::<Report>(recorder::LAST_REVIEW) {
                *shared.last_review.lock().unwrap() = Some(saved);
            }

            if storage::read_json::<bool>(ONBOARDING_CACHE).unwrap_or(false) {
                *shared.onboarding.lock().unwrap() = true;
            }

            if let Some(saved) = storage::read_json::<OverlayPrefs>(PREFS_CACHE) {
                tracing::info!(
                    escondidas = saved.hidden.len(),
                    "preferências do overlay carregadas"
                );
                *shared.overlay_prefs.write().unwrap() = saved;
            }

            app.manage(shared.clone());

            overlay::prepare(app.handle());
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                let shortcuts = app.global_shortcut();
                for shortcut in [overlay::toggle_shortcut(), overlay::interact_shortcut()] {
                    if let Err(err) = shortcuts.register(shortcut) {
                        tracing::warn!(%err, "atalho global já em uso por outro app");
                    }
                }
            }

            tauri::async_runtime::spawn(watcher::run(app.handle().clone(), shared.clone()));

            let handle = app.handle().clone();
            let champions_shared = shared.clone();
            tauri::async_runtime::spawn(async move {
                match ddragon::load_champions("pt_BR").await {
                    Ok(index) => {
                        tracing::info!(
                            version = %index.version,
                            campeoes = index.by_id.len(),
                            "campeões carregados"
                        );
                        *champions_shared.champions.write().unwrap() = index;
                    }
                    Err(err) => tracing::error!(%err, "sem dados de campeão, motor fica inativo"),
                }

                match ddragon::load_items("pt_BR").await {
                    Ok(index) => {
                        tracing::info!(itens = index.by_id.len(), "itens carregados");
                        *champions_shared.items.write().unwrap() = index;
                    }
                    Err(err) => tracing::error!(%err, "sem dados de item, build adaptativa inativa"),
                }

                match ddragon::load_runes("pt_BR").await {
                    Ok(index) => {
                        tracing::info!(runas = index.by_key.len(), "runas carregadas");
                        *champions_shared.runes.write().unwrap() = index;
                    }
                    Err(err) => tracing::warn!(%err, "sem dados de runa, sugestão de runa inativa"),
                }

                let _ = handle.emit(EVENT_ENGINE, engine_status_of(&champions_shared));
            });

            let handle = app.handle().clone();
            let profile_shared = shared.clone();
            tauri::async_runtime::spawn(async move {
                match profile::refresh(profile_shared.clone()).await {
                    Ok(stats) => {
                        tracing::info!(jogos = stats.total_games(), "histórico pessoal carregado");
                        rebuild_stats(&profile_shared, stats);
                    }
                    Err(err) => tracing::info!(%err, "componente pessoal desligado"),
                }
                let _ = handle.emit(EVENT_ENGINE, engine_status_of(&profile_shared));
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            engine_status,
            rank_draft,
            minimax_draft,
            ban_suggestions,
            get_pool,
            set_pool,
            get_live,
            build_suggestions,
            lane_analysis,
            skirmish_focus,
            gold_report,
            last_review,
            pool_suggestions,
            rune_plan,
            get_overlay_prefs,
            set_overlay_prefs,
            lcu_profile,
            onboarding_done,
            finish_onboarding
        ])
}
