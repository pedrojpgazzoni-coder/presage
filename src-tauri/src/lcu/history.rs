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
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawParticipant {
    champion_id: i64,
    stats: RawStats,
    timeline: RawTimeline,
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

    #[test]
    fn conta_sem_ranqueada_nao_vira_rank_em_branco() {
        let bruto = r#"{"queueMap":{"RANKED_SOLO_5x5":{"tier":"","division":"NA","wins":0,"losses":0}}}"#;
        let stats: RankedStats = serde_json::from_str(bruto).unwrap();
        let entrada = stats.queue_map.get("RANKED_SOLO_5x5").unwrap();
        assert!(entrada.tier.is_empty(), "e daqui que o `rank()` devolve None");
    }
}
