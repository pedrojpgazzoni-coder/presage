//! Traduz o que o cliente entrega para as perguntas do motor: chance por rota
//! e chance em cada escaramuça.
//!
//! O trabalho de verdade aqui é a rota do inimigo. No seu time o cliente diz a
//! posição de cada um; no time adversário, quase nunca. Sem ela não há par de
//! rota, e sem par não há análise.
//!
//! A saída disso é parear pela ordem do draft quando o cliente cala — é o que
//! dá para fazer — e marcar a resposta inteira como presumida, para a interface
//! dizer isso em vez de mostrar os dois casos com a mesma cara.

use serde::Serialize;

use presage_core::draft::{DraftSlot, DraftState};
use presage_core::engine::champion::{ChampionIndex, Role};
use presage_core::engine::items::ItemIndex;
use presage_core::engine::lanes::{self, LaneOutlook, Lineup, Skirmish};
use presage_core::engine::stats::StatsStore;
use presage_core::live::LiveGame;

/// Nível de referência antes da partida começar. É no 6 que as rotas se
/// resolvem, e é igual para os dez, então a comparação continua justa.
const PREGAME_LEVEL: u8 = 6;

/// A ordem em que as rotas aparecem quando precisamos presumi-las pela ordem
/// do draft. Não é verdade; é a convenção mais comum.
const DRAFT_ORDER: [Role; 5] = [Role::Top, Role::Jungle, Role::Mid, Role::Bot, Role::Support];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaneReport {
    /// Os cinco contra os cinco. `None` enquanto os lados não estão do mesmo
    /// tamanho.
    pub team: Option<lanes::TeamOutlook>,
    pub lanes: Vec<LaneOutlook>,
    pub skirmishes: Vec<Skirmish>,
    /// `draft` ou `partida`.
    pub source: &'static str,
    /// Verdadeiro quando alguma rota saiu da ordem do draft em vez de vir do
    /// cliente. A interface precisa dizer isso.
    pub roles_assumed: bool,
}

/// Monta os dois lados a partir da partida em andamento.
///
/// Aqui a posição vem da Live Client Data API para os dez, então nada é
/// presumido — e os níveis e itens são os reais.
fn from_live(game: &LiveGame) -> Option<(Vec<Lineup>, Vec<Lineup>, bool)> {
    let me = game.me()?;

    let mut ours = Vec::new();
    let mut theirs = Vec::new();
    let mut assumed = false;

    for player in &game.players {
        let role = Role::from_lcu(&player.position);
        if role == Role::Unknown {
            assumed = true;
            continue;
        }
        let entry = Lineup::new(
            role,
            player.champion_id,
            player.level,
            player.items.clone(),
        );
        if player.team == me.team {
            ours.push(entry);
        } else {
            theirs.push(entry);
        }
    }

    Some((ours, theirs, assumed))
}

/// Monta os dois lados a partir do champ select.
///
/// Campeão ainda não travado fica de fora: hover do inimigo não chega até nós,
/// e inventar um pick para completar a rota seria pior que deixar a linha em
/// branco.
fn from_draft(draft: &DraftState) -> (Vec<Lineup>, Vec<Lineup>, bool) {
    let mut assumed = false;

    let mut side = |slots: &[DraftSlot]| -> Vec<Lineup> {
        slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.champion_id > 0)
            .map(|(i, slot)| {
                let mut role = Role::from_lcu(&slot.position);
                if role == Role::Unknown {
                    // O cliente não informou. Cai na ordem do draft e avisa.
                    assumed = true;
                    role = DRAFT_ORDER[i.min(DRAFT_ORDER.len() - 1)];
                }
                Lineup::new(role, slot.champion_id, PREGAME_LEVEL, Vec::new())
            })
            .collect()
    };

    let ours = side(&draft.my_team);
    let theirs = side(&draft.their_team);
    (ours, theirs, assumed)
}

/// A análise completa, da fonte mais confiável disponível.
///
/// A partida em andamento sempre ganha do draft: lá os itens e os níveis são
/// reais, e aqui tudo é projeção.
pub fn report(
    live: Option<&LiveGame>,
    draft: Option<&DraftState>,
    champions: &ChampionIndex,
    items: &ItemIndex,
    store: &dyn StatsStore,
) -> Option<LaneReport> {
    let (ours, theirs, assumed, source) = match live.and_then(from_live) {
        Some((a, b, assumed)) => (a, b, assumed, "partida"),
        None => {
            let draft = draft?;
            let (a, b, assumed) = from_draft(draft);
            (a, b, assumed, "draft")
        }
    };

    if ours.is_empty() || theirs.is_empty() {
        return None;
    }

    Some(LaneReport {
        team: lanes::team_outlook(&ours, &theirs, champions, items),
        lanes: lanes::lane_outlooks(&ours, &theirs, champions, items, store),
        skirmishes: lanes::skirmishes(&ours, &theirs, champions, items),
        source,
        roles_assumed: assumed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use presage_core::engine::stats::NoStats;
    use presage_core::live::{LivePlayer, RawScores};

    fn champions() -> ChampionIndex {
        use presage_core::engine::champion::{BaseStats, Champion};
        let mut by_id = std::collections::HashMap::new();
        for id in 1..=10 {
            by_id.insert(
                id,
                Champion {
                    id,
                    key: format!("C{id}"),
                    name: format!("C{id}"),
                    tags: vec!["Fighter".into()],
                    attack: 6.0,
                    magic: 4.0,
                    defense: 5.0,
                    attack_range: 150.0,
                    base: BaseStats {
                        health: 600.0,
                        health_per_level: 100.0,
                        armor: 30.0,
                        armor_per_level: 4.0,
                        magic_resist: 32.0,
                        magic_resist_per_level: 2.0,
                        attack_damage: 60.0,
                        attack_damage_per_level: 3.0,
                    },
                },
            );
        }
        ChampionIndex {
            version: "test".into(),
            by_id,
        }
    }

    fn slot(position: &str, champion_id: i64) -> DraftSlot {
        DraftSlot {
            cell_id: 0,
            position: position.into(),
            champion_id,
            intent_id: 0,
            spell1: 0,
            spell2: 0,
            is_local: false,
            is_acting: false,
            locked: champion_id > 0,
        }
    }

    fn draft(mine: Vec<DraftSlot>, theirs: Vec<DraftSlot>) -> DraftState {
        DraftState {
            phase: "BAN_PICK".into(),
            time_left_ms: 20_000,
            total_time_ms: 30_000,
            local_cell_id: 0,
            local_position: "middle".into(),
            my_team: mine,
            their_team: theirs,
            my_bans: vec![],
            their_bans: vec![],
            current_action: Some("pick".into()),
            is_local_turn: true,
            unavailable: vec![],
        }
    }

    fn live_player(position: &str, id: i64, team: &str, local: bool, level: u8) -> LivePlayer {
        LivePlayer {
            champion_key: format!("C{id}"),
            champion_id: id,
            display_name: format!("C{id}"),
            team: team.into(),
            position: position.into(),
            level,
            items: vec![],
            gold_invested: 0.0,
            is_dead: false,
            respawn_timer: 0.0,
            is_local: local,
            scores: RawScores::default(),
        }
    }

    #[test]
    fn a_partida_em_andamento_ganha_do_draft() {
        let game = LiveGame {
            game_time: 900.0,
            players: vec![
                live_player("MIDDLE", 1, "ORDER", true, 12),
                live_player("MIDDLE", 2, "CHAOS", false, 12),
            ],
        };
        let d = draft(vec![slot("middle", 7)], vec![slot("middle", 8)]);

        let r = report(Some(&game), Some(&d), &champions(), &ItemIndex::default(), &NoStats).unwrap();

        assert_eq!(r.source, "partida");
        assert_eq!(r.lanes[0].ally, 1, "devia usar o campeão da partida, não o do draft");
    }

    #[test]
    fn sem_partida_usa_o_draft() {
        let d = draft(vec![slot("middle", 7)], vec![slot("middle", 8)]);
        let r = report(None, Some(&d), &champions(), &ItemIndex::default(), &NoStats).unwrap();

        assert_eq!(r.source, "draft");
        assert!(!r.roles_assumed);
        assert_eq!(r.lanes[0].ally, 7);
    }

    #[test]
    fn rota_inimiga_em_branco_e_presumida_e_marcada() {
        // O caso real: o cliente informa a sua posição e cala a deles.
        let d = draft(
            vec![slot("top", 1), slot("jungle", 2), slot("middle", 3)],
            vec![slot("", 4), slot("", 5), slot("", 6)],
        );
        let r = report(None, Some(&d), &champions(), &ItemIndex::default(), &NoStats).unwrap();

        assert!(r.roles_assumed, "a interface precisa poder dizer que presumiu");
        assert_eq!(r.lanes.len(), 3);
    }

    #[test]
    fn campeao_ainda_nao_travado_fica_de_fora() {
        let d = draft(
            vec![slot("middle", 7), slot("top", 0)],
            vec![slot("middle", 8)],
        );
        let r = report(None, Some(&d), &champions(), &ItemIndex::default(), &NoStats).unwrap();

        assert_eq!(r.lanes.len(), 1, "slot vazio não pode virar rota");
    }

    #[test]
    fn sem_draft_e_sem_partida_nao_ha_resposta() {
        assert!(report(None, None, &champions(), &ItemIndex::default(), &NoStats).is_none());
    }

    #[test]
    fn draft_so_de_um_lado_nao_rende_analise() {
        let d = draft(vec![slot("middle", 7)], vec![]);
        assert!(report(None, Some(&d), &champions(), &ItemIndex::default(), &NoStats).is_none());
    }

    #[test]
    fn partida_sem_jogador_local_cai_para_o_draft() {
        let game = LiveGame {
            game_time: 900.0,
            players: vec![live_player("MIDDLE", 1, "ORDER", false, 12)],
        };
        let d = draft(vec![slot("middle", 7)], vec![slot("middle", 8)]);

        let r = report(Some(&game), Some(&d), &champions(), &ItemIndex::default(), &NoStats).unwrap();
        assert_eq!(r.source, "draft");
    }

    #[test]
    fn antes_da_partida_todos_entram_no_mesmo_nivel() {
        // Sem isto a conta compararia níveis inventados e a vantagem viria do
        // nada.
        let d = draft(
            vec![slot("top", 1), slot("middle", 3)],
            vec![slot("top", 2), slot("middle", 4)],
        );
        let r = report(None, Some(&d), &champions(), &ItemIndex::default(), &NoStats).unwrap();

        assert!(r
            .lanes
            .iter()
            .all(|l| !l.reasons.iter().any(|m| m.contains("Nível"))));
    }

    #[test]
    fn o_time_completo_rende_as_escaramucas() {
        let posicoes = ["top", "jungle", "middle", "bottom", "utility"];
        let d = draft(
            posicoes
                .iter()
                .enumerate()
                .map(|(i, p)| slot(p, i as i64 + 1))
                .collect(),
            posicoes
                .iter()
                .enumerate()
                .map(|(i, p)| slot(p, i as i64 + 6))
                .collect(),
        );
        let r = report(None, Some(&d), &champions(), &ItemIndex::default(), &NoStats).unwrap();

        assert_eq!(r.lanes.len(), 5);
        assert!(r.skirmishes.len() >= 4);
        assert!(r.skirmishes.iter().all(|s| (0.0..=1.0).contains(&s.odds)));
    }
}
