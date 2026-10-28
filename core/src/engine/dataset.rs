//! Base de partidas agregada: winrate por rota, matchup e sinergia.
//!
//! O arquivo não vem no instalador. Winrate base, matchup e sinergia precisam
//! de milhões de partidas de milhares de contas — uma conta sozinha não produz
//! nenhum dos três, e inventar números daria um app que parece funcionar e
//! recomenda errado.
//!
//! O que existe aqui é o encaixe: um formato, um carregador e o teste de que
//! base ausente devolve `None` em vez de palpite. No dia em que o pipeline de
//! ingestão rodar, basta gravar o arquivo no lugar certo — nada mais muda.
//!
//! As chaves são texto porque JSON só tem chave de texto. `103:Mid`,
//! `103:238:Mid`, `64:103` (o par de sinergia sempre com o menor id antes, para
//! Lee+Ahri e Ahri+Lee caírem na mesma entrada).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::champion::Role;
use super::stats::{PersonalStats, Record, StatsStore};

// `default` no struct inteiro: um arquivo truncado, de uma versão mais velha
// do pipeline ou escrito à mão para teste tem de carregar. Campo que falta vira
// vazio, e `is_usable` decide se aquilo serve — melhor que recusar o arquivo e
// cair sem base nenhuma por causa de uma chave ausente.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Dataset {
    /// Patch de onde as partidas saíram. Base de patch antigo recomenda build
    /// antiga, então a interface mostra isto.
    pub patch: String,
    pub region: String,
    pub tier: String,
    /// ISO 8601.
    pub generated_at: String,
    /// Quantas partidas entraram na agregação.
    pub matches: u64,
    #[serde(default)]
    pub base: HashMap<String, Record>,
    #[serde(default)]
    pub matchup: HashMap<String, Record>,
    #[serde(default)]
    pub synergy: HashMap<String, Record>,
    /// `103:Mid:8214` — campeão, rota e a runa principal.
    #[serde(default)]
    pub runes: HashMap<String, Record>,
    /// `103:Mid:3089` — campeão, rota e um item do inventário final.
    #[serde(default)]
    pub items: HashMap<String, Record>,
}

/// Prefixo `campeão:rota:`, para varrer tudo de um campeão numa rota sem
/// guardar um índice separado. A chave é texto porque JSON só tem chave de
/// texto, e varrer um HashMap de algumas dezenas de milhares de entradas custa
/// menos que manter duas estruturas em sincronia.
fn prefixo(champion: i64, role: Role) -> String {
    format!("{champion}:{role:?}:")
}

/// Lê o número depois do último `:`.
fn sufixo(chave: &str) -> Option<i64> {
    chave.rsplit(':').next()?.parse().ok()
}

pub fn rune_key(champion: i64, role: Role, keystone: i64) -> String {
    format!("{champion}:{role:?}:{keystone}")
}

pub fn item_key(champion: i64, role: Role, item: i64) -> String {
    format!("{champion}:{role:?}:{item}")
}

pub fn base_key(champion: i64, role: Role) -> String {
    format!("{champion}:{role:?}")
}

pub fn matchup_key(champion: i64, opponent: i64, role: Role) -> String {
    format!("{champion}:{opponent}:{role:?}")
}

/// Sinergia não tem ordem: o par é sempre guardado com o menor id antes.
pub fn synergy_key(a: i64, b: i64) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    format!("{lo}:{hi}")
}

impl Dataset {
    pub fn is_usable(&self) -> bool {
        self.matches > 0 && !self.base.is_empty()
    }
}

impl StatsStore for Dataset {
    fn base(&self, champion: i64, role: Role) -> Option<Record> {
        self.base.get(&base_key(champion, role)).copied()
    }
    fn matchup(&self, champion: i64, opponent: i64, role: Role) -> Option<Record> {
        self.matchup
            .get(&matchup_key(champion, opponent, role))
            .copied()
    }
    fn synergy(&self, champion: i64, ally: i64) -> Option<Record> {
        self.synergy.get(&synergy_key(champion, ally)).copied()
    }
    fn personal(&self, _champion: i64) -> Option<Record> {
        None
    }

    fn rune(&self, champion: i64, role: Role, keystone: i64) -> Option<Record> {
        self.runes.get(&rune_key(champion, role, keystone)).copied()
    }

    fn runes_for(&self, champion: i64, role: Role) -> Vec<(i64, Record)> {
        colher(&self.runes, champion, role)
    }

    fn item(&self, champion: i64, role: Role, item: i64) -> Option<Record> {
        self.items.get(&item_key(champion, role, item)).copied()
    }

    fn items_for(&self, champion: i64, role: Role) -> Vec<(i64, Record)> {
        colher(&self.items, champion, role)
    }

    fn has_global(&self) -> bool {
        self.is_usable()
    }
    fn has_runes(&self) -> bool {
        !self.runes.is_empty()
    }
    fn has_items(&self) -> bool {
        !self.items.is_empty()
    }
}

/// Tudo de um campeão numa rota, ordenado da maior amostra para a menor.
///
/// A ordem é por amostra, não por winrate: quem chama decide o critério, e
/// devolver "a melhor" aqui esconderia que a segunda tem dez vezes mais
/// partidas.
fn colher(mapa: &HashMap<String, Record>, champion: i64, role: Role) -> Vec<(i64, Record)> {
    let chave = prefixo(champion, role);
    let mut saida: Vec<(i64, Record)> = mapa
        .iter()
        .filter(|(k, _)| k.starts_with(&chave))
        .filter_map(|(k, r)| sufixo(k).map(|id| (id, *r)))
        .collect();
    saida.sort_by(|a, b| b.1.games.cmp(&a.1.games).then(a.0.cmp(&b.0)));
    saida
}

/// O que o app usa de verdade: a base agregada (quando existe) para os três
/// componentes globais, e o histórico da conta para o componente pessoal.
///
/// Os dois são independentes de propósito. Ter a conta ligada não produz
/// winrate de matchup, e ter a base não diz nada sobre como *você* joga.
#[derive(Debug, Clone, Default)]
pub struct Layered {
    pub global: Option<Dataset>,
    pub personal: PersonalStats,
}

impl Layered {
    pub fn with_global(mut self, dataset: Dataset) -> Self {
        self.global = dataset.is_usable().then_some(dataset);
        self
    }

    pub fn patch(&self) -> Option<&str> {
        self.global.as_ref().map(|d| d.patch.as_str())
    }

    pub fn matches(&self) -> u64 {
        self.global.as_ref().map_or(0, |d| d.matches)
    }
}

impl StatsStore for Layered {
    fn base(&self, champion: i64, role: Role) -> Option<Record> {
        self.global.as_ref()?.base(champion, role)
    }
    fn matchup(&self, champion: i64, opponent: i64, role: Role) -> Option<Record> {
        self.global.as_ref()?.matchup(champion, opponent, role)
    }
    fn synergy(&self, champion: i64, ally: i64) -> Option<Record> {
        self.global.as_ref()?.synergy(champion, ally)
    }
    fn personal(&self, champion: i64) -> Option<Record> {
        self.personal.personal(champion)
    }
    fn rune(&self, champion: i64, role: Role, keystone: i64) -> Option<Record> {
        self.global.as_ref()?.rune(champion, role, keystone)
    }
    fn runes_for(&self, champion: i64, role: Role) -> Vec<(i64, Record)> {
        self.global
            .as_ref()
            .map(|d| d.runes_for(champion, role))
            .unwrap_or_default()
    }
    fn item(&self, champion: i64, role: Role, item: i64) -> Option<Record> {
        self.global.as_ref()?.item(champion, role, item)
    }
    fn items_for(&self, champion: i64, role: Role) -> Vec<(i64, Record)> {
        self.global
            .as_ref()
            .map(|d| d.items_for(champion, role))
            .unwrap_or_default()
    }
    fn has_global(&self) -> bool {
        self.global.is_some()
    }
    fn has_runes(&self) -> bool {
        self.global.as_ref().is_some_and(|d| d.has_runes())
    }
    fn has_items(&self) -> bool {
        self.global.as_ref().is_some_and(|d| d.has_items())
    }
    fn has_personal(&self) -> bool {
        self.personal.has_personal()
    }
    fn personal_games(&self) -> u32 {
        self.personal.personal_games()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cheio() -> Dataset {
        Dataset {
            patch: "16.21".into(),
            region: "BR1".into(),
            tier: "EMERALD+".into(),
            generated_at: "2026-10-27T00:00:00Z".into(),
            matches: 1_200_000,
            base: HashMap::from([(base_key(103, Role::Mid), Record::new(40_000, 21_000))]),
            matchup: HashMap::from([(
                matchup_key(103, 238, Role::Mid),
                Record::new(1840, 1050),
            )]),
            synergy: HashMap::from([(synergy_key(103, 64), Record::new(900, 480))]),
            runes: HashMap::from([
                // Eletrocutar, muito jogada; Cometa, menos jogada e com winrate
                // melhor. É de propósito: é o caso que separa "mais comum" de
                // "melhor".
                (rune_key(103, Role::Mid, 8112), Record::new(26_000, 13_000)),
                (rune_key(103, Role::Mid, 8229), Record::new(9_000, 4_950)),
            ]),
            items: HashMap::from([
                (item_key(103, Role::Mid, 3089), Record::new(31_000, 18_000)),
                (item_key(103, Role::Mid, 3157), Record::new(22_000, 12_100)),
            ]),
        }
    }

    #[test]
    fn as_runas_de_um_campeao_saem_por_amostra() {
        let d = cheio();
        let runas = d.runes_for(103, Role::Mid);

        assert_eq!(runas.len(), 2);
        assert_eq!(runas[0].0, 8112, "a mais jogada vem primeiro");
        assert_eq!(runas[0].1, Record::new(26_000, 13_000));
        assert_eq!(runas[1].0, 8229);
    }

    #[test]
    fn a_mais_jogada_nao_e_sempre_a_melhor() {
        // A razão de `runes_for` devolver tudo em vez de "a melhor": quem
        // escolhe o critério é quem chama.
        let d = cheio();
        let runas = d.runes_for(103, Role::Mid);

        let mais_jogada = runas[0].1;
        let menos_jogada = runas[1].1;
        assert!(menos_jogada.rate().unwrap() > mais_jogada.rate().unwrap());
    }

    #[test]
    fn runa_de_outra_rota_nao_vaza() {
        let d = cheio();
        assert!(d.runes_for(103, Role::Top).is_empty());
        assert!(d.rune(103, Role::Top, 8112).is_none());
        assert_eq!(d.rune(103, Role::Mid, 8112), Some(Record::new(26_000, 13_000)));
    }

    #[test]
    fn os_itens_saem_do_mesmo_jeito() {
        let d = cheio();
        let itens = d.items_for(103, Role::Mid);
        assert_eq!(itens[0].0, 3089);
        assert_eq!(d.item(103, Role::Mid, 3157), Some(Record::new(22_000, 12_100)));
    }

    #[test]
    fn base_sem_runas_responde_vazio_em_vez_de_palpite() {
        let mut d = cheio();
        d.runes.clear();
        d.items.clear();

        assert!(!d.has_runes());
        assert!(!d.has_items());
        assert!(d.runes_for(103, Role::Mid).is_empty());
        assert!(d.rune(103, Role::Mid, 8112).is_none());
        // E a base continua servindo para o resto.
        assert!(d.has_global());
    }

    #[test]
    fn base_antiga_carrega_sem_as_chaves_novas() {
        // Uma base gerada antes de o coletor guardar runas: tem de abrir.
        let antiga = r#"{"patch":"16.20","matches":10,"base":{"103:Mid":{"games":10,"wins":6}}}"#;
        let d: Dataset = serde_json::from_str(antiga).unwrap();

        assert!(d.is_usable());
        assert!(!d.has_runes());
        assert!(d.runes.is_empty());
    }

    #[test]
    fn base_vazia_nao_responde_nada() {
        let vazia = Dataset::default();
        assert!(!vazia.has_global());
        assert!(vazia.base(103, Role::Mid).is_none());
        assert!(vazia.matchup(103, 238, Role::Mid).is_none());
        assert!(vazia.synergy(103, 64).is_none());
    }

    #[test]
    fn base_com_partidas_responde() {
        let d = cheio();
        assert!(d.has_global());
        assert_eq!(d.matchup(103, 238, Role::Mid), Some(Record::new(1840, 1050)));
    }

    #[test]
    fn a_sinergia_nao_depende_da_ordem_do_par() {
        let d = cheio();
        assert_eq!(d.synergy(103, 64), d.synergy(64, 103));
        assert!(d.synergy(103, 64).is_some());
    }

    #[test]
    fn matchup_e_direcional_ao_contrario_da_sinergia() {
        let d = cheio();
        assert!(d.matchup(103, 238, Role::Mid).is_some());
        assert!(d.matchup(238, 103, Role::Mid).is_none());
    }

    #[test]
    fn a_rota_faz_parte_da_chave() {
        let d = cheio();
        assert!(d.base(103, Role::Mid).is_some());
        assert!(d.base(103, Role::Top).is_none());
    }

    #[test]
    fn base_que_diz_ter_partidas_mas_esta_vazia_nao_conta() {
        // Arquivo truncado ou pipeline que falhou no meio. Melhor tratar como
        // ausente do que recomendar a partir de quatro linhas.
        let mentirosa = Dataset {
            matches: 9_000_000,
            ..Default::default()
        };
        assert!(!mentirosa.is_usable());
        assert!(!mentirosa.has_global());
    }

    #[test]
    fn as_camadas_sao_independentes() {
        let mut pessoal = PersonalStats::default();
        pessoal.add_match("BR1_1", 103, true);

        let so_pessoal = Layered {
            global: None,
            personal: pessoal.clone(),
        };
        assert!(so_pessoal.has_personal());
        assert!(!so_pessoal.has_global());
        // Ter a conta ligada não produz matchup.
        assert!(so_pessoal.matchup(103, 238, Role::Mid).is_none());

        let so_global = Layered::default().with_global(cheio());
        assert!(so_global.has_global());
        assert!(!so_global.has_personal());
        // E ter a base não diz nada sobre como você joga.
        assert!(so_global.personal(103).is_none());
    }

    #[test]
    fn as_duas_camadas_juntas_respondem_as_duas_perguntas() {
        let mut pessoal = PersonalStats::default();
        pessoal.add_match("BR1_1", 103, true);

        let tudo = Layered {
            global: None,
            personal: pessoal,
        }
        .with_global(cheio());

        assert!(tudo.has_global() && tudo.has_personal());
        assert!(tudo.matchup(103, 238, Role::Mid).is_some());
        assert_eq!(tudo.personal(103), Some(Record::new(1, 1)));
        assert_eq!(tudo.patch(), Some("16.21"));
        assert_eq!(tudo.matches(), 1_200_000);
    }

    #[test]
    fn base_inutilizavel_nao_e_aceita_como_global() {
        let camada = Layered::default().with_global(Dataset {
            matches: 10,
            ..Default::default()
        });
        assert!(!camada.has_global());
        assert_eq!(camada.patch(), None);
    }

    #[test]
    fn o_formato_sobrevive_a_ida_e_volta_em_json() {
        let original = cheio();
        let texto = serde_json::to_string(&original).unwrap();
        let voltou: Dataset = serde_json::from_str(&texto).unwrap();

        assert_eq!(voltou.matches, original.matches);
        assert_eq!(voltou.matchup(103, 238, Role::Mid), Some(Record::new(1840, 1050)));
    }

    #[test]
    fn arquivo_so_com_o_cabecalho_carrega_sem_quebrar() {
        let voltou: Dataset =
            serde_json::from_str(r#"{"patch":"16.21","matches":0}"#).unwrap();
        assert!(!voltou.is_usable());
        assert!(voltou.base.is_empty());
    }
}
