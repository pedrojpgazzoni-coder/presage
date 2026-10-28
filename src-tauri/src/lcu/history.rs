//! Perfil e histórico de partidas, lidos do cliente local.
//!
//! # Por que isto não precisa da chave da Riot
//!
//! O LCU é o próprio cliente do jogador rodando na máquina dele. Ele já tem o
//! histórico porque é o que desenha a tela de partidas, e serve isso em
//! `127.0.0.1` para quem tiver o token do lockfile — que só existe nessa
//! máquina. Não é a Riot API; é o jogador consultando os próprios dados.
//!
//! Isso resolve "identifique meu nick e minhas partidas" hoje, sem esperar
//! chave de produção. O que continua dependendo de chave é o agregado de
//! *outros* jogadores: winrate de campeão, matchup, sinergia.
//!
//! # Sobre a forma dos dados
//!
//! O LCU não é documentado nem versionado: a Riot muda campo sem aviso. Por
//! isso tudo aqui é `Option`/`default` e nada é obrigatório — um campo que
//! sumiu vira ausência na tela, não uma tela quebrada.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::client::LcuClient;

/// Quantas partidas pedir por vez. O cliente aceita bem mais, mas a tela não
/// mostra mais que isso e cada partida carrega dez participantes.
const PAGINA: usize = 20;

// ---- o que o LCU devolve ----

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RankedStats {
    pub queue_map: std::collections::HashMap<String, QueueEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueEntry {
    pub queue_type: String,
    pub tier: String,
    pub division: String,
    pub league_points: i64,
    pub wins: i64,
    pub losses: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HistoryEnvelope {
    games: GameList,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct GameList {
    games: Vec<RawGame>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawGame {
    game_id: i64,
    game_creation: i64,
    game_duration: i64,
    queue_id: i64,
    game_mode: String,
    participants: Vec<RawParticipant>,
    /// Quem é cada participante. Vem numa lista separada, ligada por
    /// `participantId` — não dentro do participante.
    participant_identities: Vec<RawIdentity>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawParticipant {
    participant_id: i64,
    /// 100 ou 200.
    team_id: i64,
    champion_id: i64,
    stats: RawStats,
    timeline: RawTimeline,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawIdentity {
    participant_id: i64,
    player: RawPlayer,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawPlayer {
    game_name: String,
    tag_line: String,
    /// Campo antigo. Ainda aparece em partidas velhas, e é o que sobra quando
    /// `gameName` vem vazio.
    summoner_name: String,
    puuid: String,
}

impl RawPlayer {
    fn riot_id(&self) -> String {
        if self.game_name.is_empty() {
            return self.summoner_name.clone();
        }
        if self.tag_line.is_empty() {
            return self.game_name.clone();
        }
        format!("{}#{}", self.game_name, self.tag_line)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawStats {
    win: bool,
    kills: i64,
    deaths: i64,
    assists: i64,
    total_minions_killed: i64,
    neutral_minions_killed: i64,
    gold_earned: i64,
    /// Aqui está o dano — que a API ao vivo não entrega, mas o histórico sim.
    total_damage_dealt_to_champions: i64,
    total_damage_taken: i64,
    vision_score: i64,
    champ_level: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawTimeline {
    lane: String,
    role: String,
}

// ---- o que entregamos ao front ----

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rank {
    pub queue: String,
    pub tier: String,
    pub division: String,
    pub points: i64,
    pub wins: i64,
    pub losses: i64,
}

impl Rank {
    pub fn winrate(&self) -> Option<f64> {
        let total = self.wins + self.losses;
        (total > 0).then(|| self.wins as f64 / total as f64)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PastGame {
    pub game_id: i64,
    /// Milissegundos desde a época, como o cliente entrega.
    pub played_at: i64,
    pub duration: i64,
    pub queue_id: i64,
    pub champion_id: i64,
    pub lane: String,
    pub win: bool,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub cs: i64,
    pub gold: i64,
    pub damage: i64,
    pub damage_taken: i64,
    pub vision: i64,
    pub level: i64,
}

impl PastGame {
    pub fn cs_per_minute(&self) -> f64 {
        let minutos = self.duration as f64 / 60.0;
        if minutos <= 0.0 {
            return 0.0;
        }
        self.cs as f64 / minutos
    }

    /// Dano por minuto. É o número que separa quem participou da briga de quem
    /// só esteve nela.
    pub fn damage_per_minute(&self) -> f64 {
        let minutos = self.duration as f64 / 60.0;
        if minutos <= 0.0 {
            return 0.0;
        }
        self.damage as f64 / minutos
    }
}

/// Tudo que a tela inicial precisa saber sobre quem está logado.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub riot_id: String,
    pub level: i64,
    pub rank: Option<Rank>,
    pub matches: Vec<PastGame>,
}

/// Monta o perfil a partir do cliente aberto.
///
/// Rank e histórico falham separado do nome: uma conta sem ranqueada, ou um
/// endpoint que a Riot mexeu, não pode apagar o nick da tela.
pub async fn profile(client: &LcuClient, quantas: usize) -> Result<Profile> {
    let eu = client.current_summoner().await?;

    let riot_id = if eu.tag_line.is_empty() {
        eu.game_name.clone()
    } else {
        format!("{}#{}", eu.game_name, eu.tag_line)
    };

    let rank = match rank(client).await {
        Ok(r) => r,
        Err(err) => {
            tracing::debug!(%err, "sem rank; seguindo sem ele");
            None
        }
    };

    let matches = match recent(client, quantas).await {
        Ok(m) => m,
        Err(err) => {
            tracing::debug!(%err, "sem histórico; seguindo sem ele");
            Vec::new()
        }
    };

    Ok(Profile {
        riot_id,
        level: eu.summoner_level,
        rank,
        matches,
    })
}

/// Rank na fila ranqueada solo, que é a que interessa.
pub async fn rank(client: &LcuClient) -> Result<Option<Rank>> {
    let stats: RankedStats = client.get_json("/lol-ranked/v1/current-ranked-stats").await?;

    Ok(stats.queue_map.get("RANKED_SOLO_5x5").and_then(|entry| {
        // Conta sem ranqueada ainda vem com tier vazio, e um rank em branco na
        // tela é pior que nenhum.
        (!entry.tier.is_empty()).then(|| Rank {
            queue: "Ranqueada Solo".into(),
            tier: entry.tier.clone(),
            division: entry.division.clone(),
            points: entry.league_points,
            wins: entry.wins,
            losses: entry.losses,
        })
    }))
}

/// As últimas partidas do jogador logado.
///
/// O cliente devolve os dez participantes de cada partida, mas só o primeiro
/// é o jogador — é o histórico *dele*, não um dump da partida.
pub async fn recent(client: &LcuClient, quantas: usize) -> Result<Vec<PastGame>> {
    let fim = quantas.clamp(1, PAGINA);
    let caminho = format!(
        "/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex={fim}"
    );

    let envelope: HistoryEnvelope = client.get_json(&caminho).await?;

    Ok(envelope
        .games
        .games
        .into_iter()
        .filter_map(|game| {
            let p = game.participants.into_iter().next()?;
            Some(PastGame {
                game_id: game.game_id,
                played_at: game.game_creation,
                duration: game.game_duration,
                queue_id: game.queue_id,
                champion_id: p.champion_id,
                lane: p.timeline.lane,
                win: p.stats.win,
                kills: p.stats.kills,
                deaths: p.stats.deaths,
                assists: p.stats.assists,
                // Tropas de rota e de selva contam juntas; separar aqui não
                // ajuda ninguém a ler a tela.
                cs: p.stats.total_minions_killed + p.stats.neutral_minions_killed,
                gold: p.stats.gold_earned,
                damage: p.stats.total_damage_dealt_to_champions,
                damage_taken: p.stats.total_damage_taken,
                vision: p.stats.vision_score,
                level: p.stats.champ_level,
            })
        })
        .collect())
}

// ---- dano de todos, depois da partida ----

/// Uma linha do placar de dano.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DamageRow {
    pub riot_id: String,
    pub champion_id: i64,
    /// 100 ou 200, como o cliente entrega.
    pub team_id: i64,
    pub ally: bool,
    pub is_me: bool,
    pub damage: i64,
    pub damage_taken: i64,
    pub gold: i64,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    /// Fatia do dano do próprio time, de 0 a 1. É o número que diz quem
    /// carregou a briga — mais que o total cru, que só reflete a duração.
    pub team_share: f64,
}

/// O dano dos dez numa partida terminada.
///
/// **Este é o lugar onde o dano de todo mundo existe.** Durante a partida ele
/// não existe em lugar nenhum da API legal: a Live Client Data API entrega
/// abates, tropas e visão, e nada de dano. Então o número real chega aqui,
/// depois do apito, vindo do histórico do próprio cliente.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DamageBoard {
    pub game_id: i64,
    pub duration: i64,
    /// Se o jogador logado venceu.
    pub win: bool,
    /// Ordenado do maior dano para o menor, os dez juntos.
    pub rows: Vec<DamageRow>,
    pub ally_total: i64,
    pub enemy_total: i64,
}

impl DamageBoard {
    /// Dano por minuto do time do jogador. Compara partidas de durações
    /// diferentes, que o total cru não compara.
    pub fn ally_per_minute(&self) -> f64 {
        let minutos = self.duration as f64 / 60.0;
        if minutos <= 0.0 {
            return 0.0;
        }
        self.ally_total as f64 / minutos
    }
}

/// Monta o placar a partir da partida crua.
///
/// Separada da chamada de rede para poder ser testada com JSON de verdade —
/// que é o único jeito honesto de testar um endpoint não documentado.
fn board_from(game: RawGame, my_puuid: &str) -> Option<DamageBoard> {
    if game.participants.is_empty() {
        return None;
    }

    // `participantId` -> quem é. Partidas antigas podem vir sem a lista; nesse
    // caso o placar sai sem nome, mas sai.
    let quem: std::collections::HashMap<i64, &RawPlayer> = game
        .participant_identities
        .iter()
        .map(|id| (id.participant_id, &id.player))
        .collect();

    // Qual time é o meu. Sem puuid conhecido, o primeiro participante é o
    // jogador — é assim que o cliente monta o histórico dele.
    let meu_time = quem
        .iter()
        .find(|(_, player)| !my_puuid.is_empty() && player.puuid == my_puuid)
        .and_then(|(pid, _)| {
            game.participants
                .iter()
                .find(|p| p.participant_id == *pid)
                .map(|p| p.team_id)
        })
        .unwrap_or_else(|| game.participants[0].team_id);

    // As somas por time vêm antes das linhas: a fatia de cada um depende do
    // total do time dele.
    let total_de = |time: i64| -> i64 {
        game.participants
            .iter()
            .filter(|p| p.team_id == time)
            .map(|p| p.stats.total_damage_dealt_to_champions)
            .sum()
    };

    let ally_total = total_de(meu_time);
    let enemy_total: i64 = game
        .participants
        .iter()
        .filter(|p| p.team_id != meu_time)
        .map(|p| p.stats.total_damage_dealt_to_champions)
        .sum();

    let mut win = false;

    let mut rows: Vec<DamageRow> = game
        .participants
        .iter()
        .map(|p| {
            let player = quem.get(&p.participant_id);
            let ally = p.team_id == meu_time;
            let is_me = player
                .map(|pl| !my_puuid.is_empty() && pl.puuid == my_puuid)
                .unwrap_or(false);

            if is_me {
                win = p.stats.win;
            }

            let time_total = if ally { ally_total } else { enemy_total };

            DamageRow {
                riot_id: player.map(|pl| pl.riot_id()).unwrap_or_default(),
                champion_id: p.champion_id,
                team_id: p.team_id,
                ally,
                is_me,
                damage: p.stats.total_damage_dealt_to_champions,
                damage_taken: p.stats.total_damage_taken,
                gold: p.stats.gold_earned,
                kills: p.stats.kills,
                deaths: p.stats.deaths,
                assists: p.stats.assists,
                team_share: if time_total > 0 {
                    p.stats.total_damage_dealt_to_champions as f64 / time_total as f64
                } else {
                    0.0
                },
            }
        })
        .collect();

    // Sem puuid não dá para saber quem sou eu, e aí a vitória é a do primeiro
    // participante — o jogador, na forma como o cliente monta o histórico.
    if !rows.iter().any(|r| r.is_me) {
        win = game.participants[0].stats.win;
    }

    rows.sort_by(|a, b| b.damage.cmp(&a.damage));

    Some(DamageBoard {
        game_id: game.game_id,
        duration: game.game_duration,
        win,
        rows,
        ally_total,
        enemy_total,
    })
}

/// O placar de dano de uma partida terminada.
pub async fn damage_board(
    client: &LcuClient,
    game_id: i64,
    my_puuid: &str,
) -> Result<Option<DamageBoard>> {
    let game: RawGame = client
        .get_json(&format!("/lol-match-history/v1/games/{game_id}"))
        .await?;

    Ok(board_from(game, my_puuid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_winrate_sai_do_rank() {
        let r = Rank {
            queue: "x".into(),
            tier: "GOLD".into(),
            division: "II".into(),
            points: 40,
            wins: 60,
            losses: 40,
        };
        assert!((r.winrate().unwrap() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn conta_sem_partida_nao_tem_winrate() {
        // Dividir por zero aqui viraria NaN na tela.
        let r = Rank {
            queue: "x".into(),
            tier: "IRON".into(),
            division: "IV".into(),
            points: 0,
            wins: 0,
            losses: 0,
        };
        assert!(r.winrate().is_none());
    }

    fn partida(duration: i64, cs: i64, damage: i64) -> PastGame {
        PastGame {
            game_id: 1,
            played_at: 0,
            duration,
            queue_id: 420,
            champion_id: 103,
            lane: "MIDDLE".into(),
            win: true,
            kills: 5,
            deaths: 2,
            assists: 7,
            cs,
            gold: 12000,
            damage,
            damage_taken: 18000,
            vision: 20,
            level: 16,
        }
    }

    #[test]
    fn tropas_e_dano_por_minuto() {
        let p = partida(1800, 180, 27000);
        assert!((p.cs_per_minute() - 6.0).abs() < 1e-9);
        assert!((p.damage_per_minute() - 900.0).abs() < 1e-9);
    }

    #[test]
    fn partida_de_duracao_zero_nao_vira_infinito() {
        // Remake ou dado corrompido não pode virar NaN na tela.
        let p = partida(0, 10, 100);
        assert_eq!(p.cs_per_minute(), 0.0);
        assert_eq!(p.damage_per_minute(), 0.0);
    }

    #[test]
    fn o_historico_aguenta_campo_que_sumiu() {
        // O LCU nao e versionado: a Riot muda campo sem aviso. Um JSON com
        // metade dos campos tem de virar partida incompleta, nao erro.
        let bruto = r#"{"games":{"games":[{"gameId":9,"participants":[{"championId":64}]}]}}"#;
        let envelope: HistoryEnvelope = serde_json::from_str(bruto).unwrap();

        assert_eq!(envelope.games.games.len(), 1);
        assert_eq!(envelope.games.games[0].participants[0].champion_id, 64);
        assert_eq!(envelope.games.games[0].game_duration, 0);
    }

    #[test]
    fn resposta_vazia_nao_quebra() {
        let envelope: HistoryEnvelope = serde_json::from_str("{}").unwrap();
        assert!(envelope.games.games.is_empty());
    }

    /// Uma partida crua de quatro participantes, dois de cada lado.
    ///
    /// Quatro e não dez porque a pergunta que os testes fazem — quem é meu
    /// time, quem sou eu, de onde sai a fatia — se responde igual com quatro.
    fn partida_crua() -> RawGame {
        fn p(pid: i64, team: i64, champ: i64, dano: i64, win: bool) -> RawParticipant {
            RawParticipant {
                participant_id: pid,
                team_id: team,
                champion_id: champ,
                stats: RawStats {
                    win,
                    kills: 5,
                    deaths: 3,
                    assists: 8,
                    total_damage_dealt_to_champions: dano,
                    total_damage_taken: dano / 2,
                    gold_earned: 11000,
                    ..Default::default()
                },
                timeline: RawTimeline::default(),
            }
        }

        fn ident(pid: i64, nome: &str, puuid: &str) -> RawIdentity {
            RawIdentity {
                participant_id: pid,
                player: RawPlayer {
                    game_name: nome.into(),
                    tag_line: "BR1".into(),
                    summoner_name: String::new(),
                    puuid: puuid.into(),
                },
            }
        }

        RawGame {
            game_id: 77,
            game_duration: 1800,
            participants: vec![
                p(1, 100, 103, 24000, true),
                p(2, 100, 64, 8000, true),
                p(3, 200, 238, 31000, false),
                p(4, 200, 412, 5000, false),
            ],
            participant_identities: vec![
                ident(1, "Eu", "puuid-eu"),
                ident(2, "Aliado", "puuid-aliado"),
                ident(3, "Inimigo", "puuid-inimigo"),
                ident(4, "Outro", "puuid-outro"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn o_placar_sai_ordenado_por_dano() {
        let placar = board_from(partida_crua(), "puuid-eu").unwrap();
        let danos: Vec<i64> = placar.rows.iter().map(|r| r.damage).collect();
        assert_eq!(danos, vec![31000, 24000, 8000, 5000]);
        assert_eq!(placar.rows.len(), 4, "os dois times no mesmo placar");
    }

    #[test]
    fn meu_time_sai_do_meu_puuid() {
        let placar = board_from(partida_crua(), "puuid-eu").unwrap();

        let eu = placar.rows.iter().find(|r| r.is_me).unwrap();
        assert_eq!(eu.riot_id, "Eu#BR1");
        assert!(eu.ally);
        assert!(placar.win, "o resultado é o meu, não o do primeiro da lista");

        assert_eq!(placar.ally_total, 32000);
        assert_eq!(placar.enemy_total, 36000);
    }

    #[test]
    fn do_outro_lado_o_placar_vira() {
        // O mesmo JSON, lido por quem jogou do outro lado.
        let placar = board_from(partida_crua(), "puuid-inimigo").unwrap();
        assert_eq!(placar.ally_total, 36000);
        assert_eq!(placar.enemy_total, 32000);
        assert!(!placar.win);
    }

    #[test]
    fn a_fatia_e_do_proprio_time() {
        let placar = board_from(partida_crua(), "puuid-eu").unwrap();

        let eu = placar.rows.iter().find(|r| r.is_me).unwrap();
        // 24000 de 32000 do meu time — não de 68000 da partida inteira.
        assert!((eu.team_share - 0.75).abs() < 1e-9);

        let soma_aliada: f64 = placar.rows.iter().filter(|r| r.ally).map(|r| r.team_share).sum();
        assert!((soma_aliada - 1.0).abs() < 1e-9, "as fatias de um time fecham em 1");
    }

    #[test]
    fn sem_puuid_o_primeiro_participante_e_o_jogador() {
        // É assim que o cliente monta o histórico dele, então é o palpite certo.
        let placar = board_from(partida_crua(), "").unwrap();
        assert_eq!(placar.ally_total, 32000);
        assert!(placar.win);
        assert!(!placar.rows.iter().any(|r| r.is_me), "sem puuid, ninguém é marcado");
    }

    #[test]
    fn partida_de_dano_zero_nao_vira_nan() {
        // Remake: ninguém bateu em ninguém.
        let mut crua = partida_crua();
        for p in &mut crua.participants {
            p.stats.total_damage_dealt_to_champions = 0;
        }

        let placar = board_from(crua, "puuid-eu").unwrap();
        assert!(placar.rows.iter().all(|r| r.team_share == 0.0));
        assert_eq!(placar.ally_per_minute(), 0.0);
    }

    #[test]
    fn partida_sem_participante_nao_vira_placar() {
        assert!(board_from(RawGame::default(), "puuid-eu").is_none());
    }

    #[test]
    fn placar_aguenta_json_incompleto() {
        // Sem `participantIdentities`: sai sem nome, mas sai.
        let bruto = r#"{"gameId":5,"participants":[
            {"participantId":1,"teamId":100,"championId":103,
             "stats":{"win":true,"totalDamageDealtToChampions":9000}}]}"#;
        let game: RawGame = serde_json::from_str(bruto).unwrap();

        let placar = board_from(game, "puuid-eu").unwrap();
        assert_eq!(placar.rows[0].riot_id, "");
        assert_eq!(placar.ally_total, 9000);
        assert!(placar.win);
    }

    #[test]
    fn nome_antigo_serve_quando_o_riot_id_falta() {
        let antigo = RawPlayer {
            game_name: String::new(),
            tag_line: String::new(),
            summoner_name: "NomeVelho".into(),
            puuid: String::new(),
        };
        assert_eq!(antigo.riot_id(), "NomeVelho");
    }

    #[test]
    fn dano_por_minuto_do_time() {
        let placar = board_from(partida_crua(), "puuid-eu").unwrap();
        // 32000 em 30 minutos.
        assert!((placar.ally_per_minute() - 1066.6666666666667).abs() < 1e-6);
    }

    #[test]
    fn conta_sem_ranqueada_nao_vira_rank_em_branco() {
        let bruto = r#"{"queueMap":{"RANKED_SOLO_5x5":{"tier":"","division":"NA","wins":0,"losses":0}}}"#;
        let stats: RankedStats = serde_json::from_str(bruto).unwrap();
        let entrada = stats.queue_map.get("RANKED_SOLO_5x5").unwrap();
        assert!(entrada.tier.is_empty(), "e daqui que o `rank()` devolve None");
    }
}
