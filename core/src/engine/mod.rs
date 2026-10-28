pub mod build;
pub mod champion;
pub mod coach;
pub mod comp;
pub mod damage;
pub mod dataset;
pub mod gold;
pub mod items;
pub mod lanes;
pub mod minimax;
pub mod poolgap;
pub mod priority;
pub mod runes;
pub mod stats;
pub mod timeline;
pub mod wincon;

use serde::Serialize;
use std::collections::HashSet;

use crate::draft::{DraftSlot, DraftState};
use champion::{Champion, ChampionIndex, Role};
use stats::{shrink, shrunk_delta, StatsStore};

const PRIOR_RATE: f64 = 0.50;

const BASE_STRENGTH: f64 = 400.0;
const MATCHUP_STRENGTH: f64 = 800.0;
const SYNERGY_STRENGTH: f64 = 1200.0;
const PERSONAL_STRENGTH: f64 = 40.0;

/// Dispersão típica de cada efeito na população. Sem amostra, é esta a nossa
/// incerteza sobre o componente — não zero.
const MATCHUP_PRIOR_SD: f64 = 0.030;
const SYNERGY_PRIOR_SD: f64 = 0.012;
const PERSONAL_PRIOR_SD: f64 = 0.035;
const COMP_SD: f64 = 0.010;

/// Pesos escolhidos à mão, calibrados para que uma correção forte valha ~1,5pp
/// de winrate. São priors, não coeficientes ajustados — saem quando a regressão
/// sobre partidas reais entrar no lugar.
const W_DAMAGE_BALANCE: f64 = 0.050;
const W_FRONTLINE: f64 = 0.016;

const MAX_REASONS: usize = 3;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Scored {
    pub champion_id: i64,
    pub score: f64,
    /// Desvio em pontos percentuais contra uma partida neutra. É este número
    /// que aparece embaixo do retrato.
    pub delta: f64,
    pub winrate: f64,
    pub confidence: f64,
    pub sample: u32,
    pub in_pool: bool,
    pub reasons: Vec<String>,
}

pub fn rank(
    draft: &DraftState,
    index: &ChampionIndex,
    store: &dyn StatsStore,
    pool: &[i64],
) -> Vec<Scored> {
    if index.is_empty() {
        return Vec::new();
    }

    let role = Role::from_lcu(&draft.local_position);
    let pool: HashSet<i64> = pool.iter().copied().collect();
    let unavailable: HashSet<i64> = draft.unavailable.iter().copied().collect();

    let allies = team_champions(&draft.my_team, true);
    let team = comp::profile(&allies, index);
    let lane_opponent = lane_opponent(draft, index);

    let mut scored: Vec<Scored> = index
        .ids()
        .into_iter()
        .filter(|id| !unavailable.contains(id))
        .filter_map(|id| index.get(id).map(|champion| (id, champion)))
        .map(|(id, champion)| {
            score_one(
                champion,
                role,
                &team,
                &allies,
                lane_opponent.as_ref(),
                store,
                pool.contains(&id),
            )
        })
        .collect();

    scored.sort_by(|a, b| {
        b.winrate
            .partial_cmp(&a.winrate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.in_pool.cmp(&a.in_pool))
            .then(a.champion_id.cmp(&b.champion_id))
    });

    scored
}

fn score_one(
    champion: &Champion,
    role: Role,
    team: &comp::TeamProfile,
    allies: &[i64],
    lane_opponent: Option<&(i64, String)>,
    store: &dyn StatsStore,
    in_pool: bool,
) -> Scored {
    let base = shrink(store.base(champion.id, role), PRIOR_RATE, BASE_STRENGTH);

    let matchup = lane_opponent.map(|(opponent, _)| {
        shrunk_delta(
            store.matchup(champion.id, *opponent, role),
            base.rate,
            MATCHUP_STRENGTH,
        )
    });

    let synergies: Vec<stats::Estimate> = allies
        .iter()
        .map(|ally| {
            shrunk_delta(
                store.synergy(champion.id, *ally),
                base.rate,
                SYNERGY_STRENGTH,
            )
        })
        .collect();

    let personal_record = store.personal(champion.id);
    let personal = shrunk_delta(personal_record, base.rate, PERSONAL_STRENGTH);

    let balance_gain = comp::damage_balance_gain(team, champion);
    let frontline_fit = if champion.is_frontline() {
        comp::frontline_need(team)
    } else {
        0.0
    };
    let comp_delta = W_DAMAGE_BALANCE * balance_gain + W_FRONTLINE * frontline_fit;

    let matchup_delta = matchup.map(|m| m.rate).unwrap_or(0.0);
    let synergy_delta: f64 = synergies.iter().map(|s| s.rate).sum();

    let winrate =
        (base.rate + matchup_delta + synergy_delta + personal.rate + comp_delta).clamp(0.25, 0.75);

    let mut variance = base.se().powi(2) + COMP_SD.powi(2);
    variance += delta_sd(matchup.as_ref(), MATCHUP_PRIOR_SD, MATCHUP_STRENGTH).powi(2);
    for synergy in &synergies {
        variance += delta_sd(Some(synergy), SYNERGY_PRIOR_SD, SYNERGY_STRENGTH).powi(2);
    }
    variance += delta_sd(Some(&personal), PERSONAL_PRIOR_SD, PERSONAL_STRENGTH).powi(2);

    let reasons = build_reasons(
        champion,
        team,
        balance_gain,
        frontline_fit,
        matchup.as_ref().zip(lane_opponent),
        personal_record,
        &personal,
        store.has_global(),
    );

    Scored {
        champion_id: champion.id,
        score: (winrate * 1000.0).round() / 10.0,
        delta: ((winrate - PRIOR_RATE) * 1000.0).round() / 10.0,
        winrate,
        confidence: (1.96 * variance.sqrt() * 1000.0).round() / 10.0,
        sample: base.games,
        in_pool,
        reasons,
    }
}

/// Incerteza de um componente expresso como desvio: parte do espalhamento
/// populacional e encolhe conforme a amostra cresce.
fn delta_sd(estimate: Option<&stats::Estimate>, prior_sd: f64, strength: f64) -> f64 {
    let games = estimate.map(|e| e.games as f64).unwrap_or(0.0);
    prior_sd * (strength / (strength + games))
}

fn build_reasons(
    champion: &Champion,
    team: &comp::TeamProfile,
    balance_gain: f64,
    frontline_fit: f64,
    matchup: Option<(&stats::Estimate, &(i64, String))>,
    personal_record: Option<stats::Record>,
    personal: &stats::Estimate,
    has_global: bool,
) -> Vec<String> {
    let mut reasons = Vec::new();

    if let Some((estimate, (_, opponent_name))) = matchup {
        if estimate.games > 0 && estimate.rate.abs() > 0.005 {
            let verbo = if estimate.rate > 0.0 { "Vence" } else { "Perde para" };
            reasons.push(format!(
                "{verbo} {opponent_name} na rota ({:+.1}pp, n={})",
                estimate.rate * 100.0,
                estimate.games
            ));
        }
    }

    if balance_gain > 0.01 {
        reasons.push(format!(
            "Equilibra o dano: {:.0}% → {:.0}% físico",
            team.physical_share * 100.0,
            comp::share_after(team, champion) * 100.0
        ));
    }

    if frontline_fit > 0.0 {
        reasons.push("Entra como a frontline que o time não tem".to_string());
    }

    if let Some(record) = personal_record {
        if record.games >= 5 {
            reasons.push(format!(
                "Seus jogos: {}V-{}D ({:+.1}pp)",
                record.wins,
                record.games - record.wins,
                personal.rate * 100.0
            ));
        }
    }

    if reasons.is_empty() && !has_global {
        reasons.push("Sem base estatística ainda — ordenado só por composição".to_string());
    }

    reasons.truncate(MAX_REASONS);
    reasons
}

fn team_champions(slots: &[DraftSlot], skip_local: bool) -> Vec<i64> {
    slots
        .iter()
        .filter(|slot| !(skip_local && slot.is_local))
        .map(|slot| {
            if slot.champion_id > 0 {
                slot.champion_id
            } else {
                slot.intent_id
            }
        })
        .filter(|id| *id > 0)
        .collect()
}

fn lane_opponent(draft: &DraftState, index: &ChampionIndex) -> Option<(i64, String)> {
    if draft.local_position.is_empty() {
        return None;
    }
    draft
        .their_team
        .iter()
        .find(|slot| slot.position == draft.local_position && slot.champion_id > 0)
        .and_then(|slot| {
            index
                .get(slot.champion_id)
                .map(|champion| (slot.champion_id, champion.name.clone()))
        })
}

#[cfg(test)]
mod tests {
    use super::champion::fake;
    use super::stats::Record;
    use super::*;
    use std::collections::HashMap;

    fn slot(cell_id: i64, position: &str, champion_id: i64, is_local: bool) -> DraftSlot {
        DraftSlot {
            cell_id,
            position: position.to_string(),
            champion_id,
            intent_id: 0,
            spell1: 4,
            spell2: 14,
            is_local,
            is_acting: false,
            locked: champion_id > 0,
        }
    }

    fn index() -> ChampionIndex {
        let champions = vec![
            fake(1, "Marks", "Marksman", 9.0, 2.0, 3.0),
            fake(2, "Lutador", "Fighter", 8.0, 3.0, 5.0),
            fake(3, "Assassino", "Assassin", 9.0, 3.0, 3.0),
            fake(4, "Mago", "Mage", 2.0, 9.0, 4.0),
            fake(5, "Tanque", "Tank", 5.0, 6.0, 9.0),
            fake(6, "Inimigo", "Mage", 3.0, 8.0, 4.0),
        ];
        ChampionIndex {
            version: "test".into(),
            by_id: champions.into_iter().map(|c| (c.id, c)).collect::<HashMap<_, _>>(),
        }
    }

    fn draft() -> DraftState {
        DraftState {
            phase: "BAN_PICK".into(),
            time_left_ms: 27_000,
            total_time_ms: 30_000,
            local_cell_id: 0,
            local_position: "middle".into(),
            my_team: vec![
                slot(0, "middle", 0, true),
                slot(1, "top", 2, false),
                slot(2, "jungle", 3, false),
                slot(3, "bottom", 1, false),
            ],
            their_team: vec![slot(5, "middle", 6, false)],
            my_bans: vec![],
            their_bans: vec![],
            current_action: Some("pick".into()),
            is_local_turn: true,
            unavailable: vec![1, 2, 3, 6],
            }
    }

    struct FakeStats;
    impl StatsStore for FakeStats {
        fn base(&self, champion: i64, _role: Role) -> Option<Record> {
            (champion == 4).then(|| Record::new(20_000, 11_000))
        }
        fn matchup(&self, champion: i64, opponent: i64, _role: Role) -> Option<Record> {
            (champion == 4 && opponent == 6).then(|| Record::new(5_000, 3_000))
        }
        fn synergy(&self, _champion: i64, _ally: i64) -> Option<Record> {
            None
        }
        fn personal(&self, _champion: i64) -> Option<Record> {
            None
        }
        fn has_global(&self) -> bool {
            true
        }
    }

    #[test]
    fn indisponiveis_ficam_de_fora() {
        let scored = rank(&draft(), &index(), &stats::NoStats, &[]);
        let mut ids: Vec<i64> = scored.iter().map(|s| s.champion_id).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![4, 5]);
    }

    #[test]
    fn frontline_pesa_mais_que_dano_em_time_sem_tanque() {
        // Aliados são Fighter + Assassin + Marksman: físico demais e sem
        // frontline. O tanque resolve o buraco maior, então vem primeiro.
        let scored = rank(&draft(), &index(), &stats::NoStats, &[]);
        assert_eq!(scored[0].champion_id, 5);
    }

    #[test]
    fn indice_vazio_nao_entra_em_panico() {
        assert!(rank(&draft(), &ChampionIndex::default(), &stats::NoStats, &[]).is_empty());
    }

    #[test]
    fn sem_estatistica_o_ranking_vem_da_composicao() {
        // Time aliado é Fighter + Assassin + Marksman: quase todo físico e sem
        // frontline. O mago equilibra o dano, o tanque preenche a frontline;
        // ambos têm de ficar acima de 50%.
        let scored = rank(&draft(), &index(), &stats::NoStats, &[]);
        for item in &scored {
            assert!(item.winrate > 0.5, "{:?}", item);
        }
    }

    #[test]
    fn sem_estatistica_a_razao_diz_isso() {
        let scored = rank(&draft(), &index(), &stats::NoStats, &[]);
        let tanque = scored.iter().find(|s| s.champion_id == 5).unwrap();
        assert!(tanque.reasons.iter().any(|r| r.contains("frontline")));
    }

    #[test]
    fn pool_e_marcado_sem_alterar_a_ordem() {
        let com_pool = rank(&draft(), &index(), &stats::NoStats, &[5]);
        let sem_pool = rank(&draft(), &index(), &stats::NoStats, &[]);
        assert!(com_pool.iter().find(|s| s.champion_id == 5).unwrap().in_pool);
        let ordem_a: Vec<i64> = com_pool.iter().map(|s| s.champion_id).collect();
        let ordem_b: Vec<i64> = sem_pool.iter().map(|s| s.champion_id).collect();
        assert_eq!(ordem_a, ordem_b);
    }

    #[test]
    fn sem_dados_o_intervalo_e_largo() {
        let scored = rank(&draft(), &index(), &stats::NoStats, &[]);
        assert!(scored[0].confidence > 8.0, "{}", scored[0].confidence);
        assert_eq!(scored[0].sample, 0);
    }

    #[test]
    fn com_dados_o_intervalo_encolhe() {
        let sem = rank(&draft(), &index(), &stats::NoStats, &[]);
        let com = rank(&draft(), &index(), &FakeStats, &[]);
        let mago_sem = sem.iter().find(|s| s.champion_id == 4).unwrap();
        let mago_com = com.iter().find(|s| s.champion_id == 4).unwrap();
        assert!(mago_com.confidence < mago_sem.confidence);
        assert_eq!(mago_com.sample, 20_000);
    }

    #[test]
    fn matchup_favoravel_vira_razao() {
        let scored = rank(&draft(), &index(), &FakeStats, &[]);
        let mago = scored.iter().find(|s| s.champion_id == 4).unwrap();
        assert!(
            mago.reasons.iter().any(|r| r.starts_with("Vence Inimigo")),
            "{:?}",
            mago.reasons
        );
    }

    #[test]
    fn historico_pessoal_entra_no_score_e_na_razao() {
        let mut pessoal = stats::PersonalStats::default();
        for i in 0..20 {
            pessoal.add_match(&format!("BR1_{i}"), 4, i < 15);
        }

        let sem = rank(&draft(), &index(), &stats::NoStats, &[]);
        let com = rank(&draft(), &index(), &pessoal, &[]);

        let mago_sem = sem.iter().find(|s| s.champion_id == 4).unwrap();
        let mago_com = com.iter().find(|s| s.champion_id == 4).unwrap();

        assert!(mago_com.winrate > mago_sem.winrate);
        assert!(
            mago_com.reasons.iter().any(|r| r.contains("15V-5D")),
            "{:?}",
            mago_com.reasons
        );
    }

    #[test]
    fn historico_pessoal_nao_preenche_matchup() {
        // Jogar muito não cria base de matchup: isso precisa de muitos
        // jogadores, não de muitas partidas suas.
        let mut pessoal = stats::PersonalStats::default();
        for i in 0..200 {
            pessoal.add_match(&format!("BR1_{i}"), 4, true);
        }
        let scored = rank(&draft(), &index(), &pessoal, &[]);
        let mago = scored.iter().find(|s| s.champion_id == 4).unwrap();

        assert_eq!(mago.sample, 0);
        assert!(!mago.reasons.iter().any(|r| r.contains("na rota")));
    }

    #[test]
    fn ordem_e_deterministica() {
        let a = rank(&draft(), &index(), &stats::NoStats, &[]);
        let b = rank(&draft(), &index(), &stats::NoStats, &[]);
        assert_eq!(a, b);
    }

    #[test]
    fn winrate_fica_dentro_dos_limites() {
        for item in rank(&draft(), &index(), &FakeStats, &[]) {
            assert!((0.25..=0.75).contains(&item.winrate), "{:?}", item);
        }
    }
}
