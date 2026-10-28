use super::champion::Role;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub games: u32,
    pub wins: u32,
}

impl Record {
    pub fn new(games: u32, wins: u32) -> Self {
        Record {
            games,
            wins: wins.min(games),
        }
    }

    /// Winrate cru, sem encolhimento. Zero partidas devolve `None` em vez de
    /// NaN ou de 0,5 — não saber não é o mesmo que estar no meio.
    ///
    /// Para ranquear o que o motor usa, prefira `shrink`: este número cru faz
    /// uma amostra de três partidas parecer tão firme quanto uma de três mil.
    pub fn rate(&self) -> Option<f64> {
        (self.games > 0).then(|| self.wins as f64 / self.games as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub rate: f64,
    pub effective_n: f64,
    pub games: u32,
}

impl Estimate {
    /// Erro padrão da proporção, usando o n efetivo (amostra + força do prior).
    pub fn se(&self) -> f64 {
        if self.effective_n <= 0.0 {
            return 0.5;
        }
        let p = self.rate.clamp(0.0, 1.0);
        (p * (1.0 - p) / self.effective_n).sqrt()
    }
}

/// Média posterior de uma Beta(prior * strength, (1 - prior) * strength)
/// atualizada pela amostra. Com zero jogos devolve o prior; com amostra grande
/// converge para a taxa observada.
pub fn shrink(record: Option<Record>, prior: f64, strength: f64) -> Estimate {
    let strength = strength.max(1.0);
    let (games, wins) = record
        .map(|r| (r.games as f64, r.wins as f64))
        .unwrap_or((0.0, 0.0));

    let effective_n = games + strength;
    let rate = ((wins + strength * prior) / effective_n).clamp(0.0, 1.0);

    Estimate {
        rate,
        effective_n,
        games: games as u32,
    }
}

/// Mesma conta, expressa como desvio em relação à linha de base. Sem amostra o
/// desvio é exatamente zero, que é o que queremos: componente sem dado não
/// mexe no ranking.
pub fn shrunk_delta(record: Option<Record>, baseline: f64, strength: f64) -> Estimate {
    let estimate = shrink(record, baseline, strength);
    Estimate {
        rate: estimate.rate - baseline,
        ..estimate
    }
}

pub trait StatsStore: Send + Sync {
    fn base(&self, champion: i64, role: Role) -> Option<Record>;
    fn matchup(&self, champion: i64, opponent: i64, role: Role) -> Option<Record>;
    fn synergy(&self, champion: i64, ally: i64) -> Option<Record>;
    fn personal(&self, champion: i64) -> Option<Record>;

    /// Quantas partidas, e quantas vitórias, com esta runa principal neste
    /// campeão e rota.
    ///
    /// Runa é a medida mais limpa que a base tem: ela é escolhida antes do
    /// primeiro minuto, sem saber o que vai acontecer. Nenhum viés de
    /// resultado entra na escolha.
    fn rune(&self, _champion: i64, _role: Role, _keystone: i64) -> Option<Record> {
        None
    }

    /// Todas as runas principais vistas neste campeão e rota, com a contagem
    /// de cada uma. É daqui que sai "a melhor runa", ordenando por winrate.
    fn runes_for(&self, _champion: i64, _role: Role) -> Vec<(i64, Record)> {
        Vec::new()
    }

    /// Partidas em que o item terminou no inventário deste campeão nesta rota.
    ///
    /// **Este número tem viés e não dá para tirar.** Quem ganha a partida tem
    /// mais ouro e termina mais itens, então item caro aparece mais em vitória
    /// por ser caro, não por ser bom. O inventário final é o que a match-v5
    /// entrega; a ordem de compra só existe na timeline, que custa uma chamada
    /// a mais por partida. Quem lê isto tem de dizer o viés na tela.
    fn item(&self, _champion: i64, _role: Role, _item: i64) -> Option<Record> {
        None
    }

    fn items_for(&self, _champion: i64, _role: Role) -> Vec<(i64, Record)> {
        Vec::new()
    }

    /// Base agregada sobre muitos jogadores: winrate por rota, matchup,
    /// sinergia. Vem do pipeline de ingestão.
    fn has_global(&self) -> bool {
        false
    }

    /// A base carrega runas. Pode ser falso numa base antiga, gerada antes de
    /// o coletor passar a guardá-las.
    fn has_runes(&self) -> bool {
        false
    }

    fn has_items(&self) -> bool {
        false
    }

    /// Histórico do próprio jogador. Vem da conta dele, e só alimenta o
    /// componente pessoal.
    fn has_personal(&self) -> bool {
        false
    }

    fn personal_games(&self) -> u32 {
        0
    }
}

pub struct NoStats;

impl StatsStore for NoStats {
    fn base(&self, _champion: i64, _role: Role) -> Option<Record> {
        None
    }
    fn matchup(&self, _champion: i64, _opponent: i64, _role: Role) -> Option<Record> {
        None
    }
    fn synergy(&self, _champion: i64, _ally: i64) -> Option<Record> {
        None
    }
    fn personal(&self, _champion: i64) -> Option<Record> {
        None
    }
}

/// Vitórias e derrotas do próprio jogador, por campeão.
///
/// Cobre só o componente pessoal, e de propósito: winrate base, matchup e
/// sinergia precisam de milhões de partidas de milhares de jogadores. Uma
/// conta sozinha não produz nenhum dos três — com 100 partidas você teria
/// menos de uma observação por matchup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersonalStats {
    pub puuid: String,
    pub by_champion: HashMap<i64, Record>,
    /// Partidas já contabilizadas, para a busca ser incremental.
    pub seen: Vec<String>,
}

impl PersonalStats {
    pub fn add_match(&mut self, match_id: &str, champion: i64, won: bool) -> bool {
        if self.seen.iter().any(|id| id == match_id) {
            return false;
        }
        self.seen.push(match_id.to_string());
        let entry = self
            .by_champion
            .entry(champion)
            .or_insert(Record { games: 0, wins: 0 });
        entry.games += 1;
        if won {
            entry.wins += 1;
        }
        true
    }

    pub fn total_games(&self) -> u32 {
        self.by_champion.values().map(|r| r.games).sum()
    }
}

impl StatsStore for PersonalStats {
    fn base(&self, _champion: i64, _role: Role) -> Option<Record> {
        None
    }
    fn matchup(&self, _champion: i64, _opponent: i64, _role: Role) -> Option<Record> {
        None
    }
    fn synergy(&self, _champion: i64, _ally: i64) -> Option<Record> {
        None
    }
    fn personal(&self, champion: i64) -> Option<Record> {
        self.by_champion.get(&champion).copied()
    }
    fn has_personal(&self) -> bool {
        !self.by_champion.is_empty()
    }
    fn personal_games(&self) -> u32 {
        self.total_games()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIOR: f64 = 0.5;

    #[test]
    fn sem_amostra_devolve_o_prior() {
        let e = shrink(None, PRIOR, 400.0);
        assert!((e.rate - PRIOR).abs() < 1e-12);
        assert_eq!(e.games, 0);
    }

    #[test]
    fn sem_amostra_o_desvio_e_zero() {
        let e = shrunk_delta(None, 0.52, 800.0);
        assert!(e.rate.abs() < 1e-12);
    }

    #[test]
    fn amostra_grande_domina_o_prior() {
        let e = shrink(Some(Record::new(100_000, 60_000)), PRIOR, 400.0);
        assert!((e.rate - 0.60).abs() < 0.01, "{}", e.rate);
    }

    #[test]
    fn amostra_pequena_fica_perto_do_prior() {
        // 10 jogos, 10 vitórias: sem shrinkage viraria 100%.
        let e = shrink(Some(Record::new(10, 10)), PRIOR, 400.0);
        assert!(e.rate < 0.52, "{}", e.rate);
    }

    #[test]
    fn erro_padrao_encolhe_com_a_amostra() {
        let pouco = shrink(Some(Record::new(50, 30)), PRIOR, 400.0);
        let muito = shrink(Some(Record::new(50_000, 30_000)), PRIOR, 400.0);
        assert!(muito.se() < pouco.se());
    }

    #[test]
    fn vitorias_acima_de_jogos_sao_cortadas() {
        let r = Record::new(10, 99);
        assert_eq!(r.wins, 10);
        let e = shrink(Some(r), PRIOR, 400.0);
        assert!((0.0..=1.0).contains(&e.rate));
    }

    #[test]
    fn forca_zero_nao_divide_por_zero() {
        let e = shrink(Some(Record::new(0, 0)), PRIOR, 0.0);
        assert!(e.rate.is_finite());
        assert!(e.se().is_finite());
    }

    #[test]
    fn nostats_nao_tem_dado() {
        assert!(!NoStats.has_global());
        assert!(!NoStats.has_personal());
        assert!(NoStats.base(103, Role::Mid).is_none());
    }

    #[test]
    fn pessoal_acumula_vitorias_e_derrotas() {
        let mut stats = PersonalStats::default();
        stats.add_match("BR1_1", 103, true);
        stats.add_match("BR1_2", 103, false);
        stats.add_match("BR1_3", 64, true);

        assert_eq!(stats.personal(103), Some(Record { games: 2, wins: 1 }));
        assert_eq!(stats.personal(64), Some(Record { games: 1, wins: 1 }));
        assert_eq!(stats.total_games(), 3);
    }

    #[test]
    fn partida_repetida_nao_conta_duas_vezes() {
        let mut stats = PersonalStats::default();
        assert!(stats.add_match("BR1_1", 103, true));
        assert!(!stats.add_match("BR1_1", 103, true));
        assert_eq!(stats.total_games(), 1);
    }

    #[test]
    fn pessoal_nao_inventa_dado_global() {
        let mut stats = PersonalStats::default();
        stats.add_match("BR1_1", 103, true);

        assert!(stats.has_personal());
        assert!(!stats.has_global());
        assert!(stats.base(103, Role::Mid).is_none());
        assert!(stats.matchup(103, 238, Role::Mid).is_none());
        assert!(stats.synergy(103, 64).is_none());
    }

    #[test]
    fn campeao_nunca_jogado_nao_tem_registro() {
        let mut stats = PersonalStats::default();
        stats.add_match("BR1_1", 103, true);
        assert!(stats.personal(777).is_none());
    }
}
