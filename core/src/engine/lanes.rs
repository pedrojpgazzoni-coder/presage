//! Chance de vitória por rota e simulação de escaramuça 2v2 / 3v3.
//!
//! Duas perguntas diferentes, mesma base:
//!
//! - *rota*: você contra o seu oponente direto, os dois sozinhos;
//! - *escaramuça*: um grupo contra o outro, quando a briga acontece.
//!
//! O número sai de duas fontes. A matemática do duelo — resistências, dano e
//! penetração reais dos dois lados — está sempre disponível, porque depende só
//! do Data Dragon e dos itens. O histórico de matchup depende da base de
//! partidas: sem ela, o componente contribui exatamente zero, e a interface diz
//! que o número veio só da conta.

use serde::Serialize;

use super::build::{
    incoming_physical_share, loadout, side_power, skirmish_odds, Combatant,
};
use super::champion::{ChampionIndex, Role};
use super::items::ItemIndex;
use super::stats::{shrunk_delta, StatsStore};
use super::timeline::Spawn;

/// A mesma força de prior usada no ranking de draft.
const MATCHUP_STRENGTH: f64 = 800.0;

/// Quanto a matemática do duelo mexe na probabilidade final.
///
/// Poder de combate não é vitória de rota: quem ganha o 1v1 em tela aberta
/// ainda perde a rota por onda, por gank e por nível. Com 0,60 um duelo de
/// 60/40 vira 56% de rota, que é a ordem de grandeza certa.
const W_DUEL: f64 = 0.60;

/// Nenhuma rota está decidida antes de começar.
const FLOOR: f64 = 0.28;
const CEILING: f64 = 0.72;

const MAX_REASONS: usize = 3;

/// As três fases do jogo, com o nível que representa cada uma: todas as
/// básicas no 3, a suprema no 6, e a segunda carga dela no 11 — daí para
/// frente a diferença entre campeões já está desenhada.
pub const BREAKPOINTS: [u8; 3] = [3, 6, 11];

/// O nome da fase, que é como as pessoas pensam o jogo. O nível aparece junto
/// na interface para o número não virar rótulo vago.
pub fn phase_label(level: u8) -> &'static str {
    match level {
        0..=4 => "early game",
        5..=9 => "mid game",
        _ => "late game",
    }
}

/// Quem ganha num nível, com os dois lados nivelados.
///
/// **O que esta curva vê:** crescimento de vida, armadura, resistência mágica
/// e dano de ataque por nível, que diferem bastante entre campeões, mais os
/// itens que cada um já tem.
///
/// **O que ela não vê:** habilidades. O modelo não sabe que alguém ganhou a
/// suprema no 6, nem quanto ela bate. Então a curva mostra a diferença de
/// *atributos* ao longo do jogo — real, mas não é o powerspike de habilidade
/// que a palavra costuma sugerir. A interface precisa dizer isso.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LevelOdds {
    pub level: u8,
    /// `early game`, `mid game`, `late game`.
    pub phase: &'static str,
    pub odds: f64,
}

fn at_level(side: &[Combatant], level: u8) -> Vec<Combatant> {
    side.iter()
        .map(|c| Combatant {
            level,
            ..c.clone()
        })
        .collect()
}

/// A chance ao longo dos níveis, com os dois lados no mesmo nível.
///
/// Nivelar os dois é o ponto: durante a partida os níveis reais já diferem, e
/// a pergunta aqui é outra — em igualdade, de quem é o jogo.
fn curve(
    ours: &[Combatant],
    theirs: &[Combatant],
    champions: &ChampionIndex,
    items: &ItemIndex,
) -> Vec<LevelOdds> {
    BREAKPOINTS
        .iter()
        .map(|&level| LevelOdds {
            level,
            phase: phase_label(level),
            odds: skirmish_odds(
                &at_level(ours, level),
                &at_level(theirs, level),
                champions,
                items,
            ),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaneOutlook {
    pub role: Role,
    pub lane: &'static str,
    pub ally: i64,
    pub enemy: i64,
    pub ally_name: String,
    pub enemy_name: String,
    /// Chance de você ganhar a rota, de 0 a 1.
    pub win_probability: f64,
    /// Desvio em pontos percentuais contra uma rota equilibrada.
    pub margin: f64,
    /// Partidas por trás do componente de matchup. Zero quando não há base.
    pub sample: u32,
    /// Falso quando o número saiu só da matemática do duelo. A interface muda
    /// o texto com isso, em vez de mostrar os dois casos iguais.
    pub from_data: bool,
    /// A mesma conta nos níveis 3, 6, 11 e 16, com os dois nivelados.
    pub by_level: Vec<LevelOdds>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skirmish {
    /// Identificador estável do agrupamento. A interface escolhe qual mostrar
    /// por este campo, não pelo texto de `detail` — texto muda, chave não.
    pub key: &'static str,
    /// `JG+TOP x JG+TOP`
    pub label: String,
    pub detail: &'static str,
    pub size: usize,
    pub ours: Vec<i64>,
    pub theirs: Vec<i64>,
    /// Chance de o seu lado ganhar, de 0 a 1.
    pub odds: f64,
    /// A mesma conta nos níveis 3, 6, 11 e 16.
    pub by_level: Vec<LevelOdds>,
    pub reasons: Vec<String>,
}

/// Os agrupamentos que acontecem de verdade numa partida: chave, rotas, texto.
const GROUPS: &[(&str, &[Role], &str)] = &[
    ("jg_top", &[Role::Jungle, Role::Top], "gank ou invade no topo"),
    ("jg_mid", &[Role::Jungle, Role::Mid], "gank ou briga no meio"),
    ("bot", &[Role::Bot, Role::Support], "rota inferior, sem jungle"),
    (
        "dragao",
        &[Role::Jungle, Role::Bot, Role::Support],
        "briga no dragão",
    ),
    (
        "arauto",
        &[Role::Jungle, Role::Top, Role::Mid],
        "briga no arauto",
    ),
];

/// Um participante com a rota já resolvida.
#[derive(Debug, Clone, PartialEq)]
pub struct Lineup {
    pub role: Role,
    pub combatant: Combatant,
}

impl Lineup {
    pub fn new(role: Role, champion_id: i64, level: u8, items: Vec<i64>) -> Self {
        Lineup {
            role,
            combatant: Combatant {
                champion_id,
                level,
                items,
            },
        }
    }
}

fn find(side: &[Lineup], role: Role) -> Option<&Lineup> {
    side.iter().find(|entry| entry.role == role)
}

fn name(id: i64, champions: &ChampionIndex) -> String {
    champions
        .get(id)
        .map(|champion| champion.name.clone())
        .unwrap_or_else(|| format!("#{id}"))
}

/// Chance de vitória em cada rota em que os dois lados são conhecidos.
///
/// Rotas sem oponente identificado ficam de fora em vez de virarem 50%: um
/// palpite com cara de medida é pior que silêncio.
pub fn lane_outlooks(
    allies: &[Lineup],
    enemies: &[Lineup],
    champions: &ChampionIndex,
    items: &ItemIndex,
    store: &dyn StatsStore,
) -> Vec<LaneOutlook> {
    let order = [Role::Top, Role::Jungle, Role::Mid, Role::Bot, Role::Support];

    order
        .iter()
        .filter_map(|&role| {
            let ally = find(allies, role)?;
            let enemy = find(enemies, role)?;
            Some(one_lane(role, ally, enemy, champions, items, store))
        })
        .collect()
}

fn one_lane(
    role: Role,
    ally: &Lineup,
    enemy: &Lineup,
    champions: &ChampionIndex,
    items: &ItemIndex,
    store: &dyn StatsStore,
) -> LaneOutlook {
    let mine = std::slice::from_ref(&ally.combatant);
    let theirs = std::slice::from_ref(&enemy.combatant);

    let duel = skirmish_odds(mine, theirs, champions, items);

    let matchup = shrunk_delta(
        store.matchup(ally.combatant.champion_id, enemy.combatant.champion_id, role),
        0.5,
        MATCHUP_STRENGTH,
    );

    // Sem amostra, `shrunk_delta` devolve exatamente 0 — o duelo responde
    // sozinho e o `from_data` abaixo diz isso à interface.
    let raw = 0.5 + W_DUEL * (duel - 0.5) + matchup.rate;
    let probability = raw.clamp(FLOOR, CEILING);

    let ally_name = name(ally.combatant.champion_id, champions);
    let enemy_name = name(enemy.combatant.champion_id, champions);

    let mut reasons = Vec::new();

    if matchup.games > 0 {
        let verb = if matchup.rate >= 0.0 { "Vence" } else { "Perde para" };
        reasons.push(format!(
            "{verb} {enemy_name} na rota ({:+.1}pp, n={})",
            matchup.rate * 100.0,
            matchup.games
        ));
    }

    reasons.extend(duel_reasons(
        duel,
        &ally_name,
        &enemy_name,
        ally,
        enemy,
        champions,
        items,
    ));
    reasons.truncate(MAX_REASONS);

    LaneOutlook {
        role,
        lane: role.short(),
        ally: ally.combatant.champion_id,
        enemy: enemy.combatant.champion_id,
        ally_name,
        enemy_name,
        win_probability: probability,
        margin: (probability - 0.5) * 100.0,
        sample: matchup.games,
        from_data: matchup.games > 0,
        by_level: curve(mine, theirs, champions, items),
        reasons,
    }
}

fn duel_reasons(
    duel: f64,
    ally_name: &str,
    enemy_name: &str,
    ally: &Lineup,
    enemy: &Lineup,
    champions: &ChampionIndex,
    items: &ItemIndex,
) -> Vec<String> {
    let mut out = Vec::new();

    // O texto muda se já há itens na mesa: antes do jogo é uma projeção em
    // igualdade, durante é o estado real.
    let armed = !ally.combatant.items.is_empty() || !enemy.combatant.items.is_empty();
    let when = if armed {
        "com os itens de agora"
    } else {
        "em igualdade de itens"
    };

    if (duel - 0.5).abs() >= 0.02 {
        let verb = if duel > 0.5 { "Ganha" } else { "Perde" };
        out.push(format!("{verb} o 1v1 {when} ({:.0}%)", duel * 100.0));
    }

    let (Some(mine), Some(theirs)) = (
        champions.get(ally.combatant.champion_id),
        champions.get(enemy.combatant.champion_id),
    ) else {
        return out;
    };

    if mine.is_ranged() && !theirs.is_ranged() {
        out.push(format!("{ally_name} tem alcance; {enemy_name} precisa chegar perto"));
    } else if theirs.is_ranged() && !mine.is_ranged() {
        out.push(format!("{enemy_name} empurra a onda de longe"));
    }

    if let (Some(a), Some(b)) = (
        loadout(&ally.combatant, champions, items),
        loadout(&enemy.combatant, champions, items),
    ) {
        if ally.combatant.level != enemy.combatant.level {
            out.push(format!(
                "Nível {} contra {}",
                ally.combatant.level, enemy.combatant.level
            ));
        } else if b.health > a.health * 1.15 {
            out.push(format!("{enemy_name} aguenta mais um golpe que você"));
        }
    }

    out
}

/// Chance de ganhar cada agrupamento que costuma brigar junto.
///
/// Um grupo só entra na lista quando os dois lados estão completos: 2v2 contra
/// 2v1 é outra pergunta, e responder como se fosse a mesma seria mentira.
pub fn skirmishes(
    allies: &[Lineup],
    enemies: &[Lineup],
    champions: &ChampionIndex,
    items: &ItemIndex,
) -> Vec<Skirmish> {
    GROUPS
        .iter()
        .filter_map(|(key, roles, detail)| {
            let ours: Vec<&Lineup> = roles.iter().filter_map(|&r| find(allies, r)).collect();
            let theirs: Vec<&Lineup> = roles.iter().filter_map(|&r| find(enemies, r)).collect();
            if ours.len() != roles.len() || theirs.len() != roles.len() {
                return None;
            }

            let mine: Vec<Combatant> = ours.iter().map(|l| l.combatant.clone()).collect();
            let foes: Vec<Combatant> = theirs.iter().map(|l| l.combatant.clone()).collect();
            let odds = skirmish_odds(&mine, &foes, champions, items);

            let tag = roles
                .iter()
                .map(|r| r.short())
                .collect::<Vec<_>>()
                .join("+");

            Some(Skirmish {
                key,
                label: format!("{tag} x {tag}"),
                detail,
                size: roles.len(),
                ours: mine.iter().map(|c| c.champion_id).collect(),
                theirs: foes.iter().map(|c| c.champion_id).collect(),
                odds,
                by_level: curve(&mine, &foes, champions, items),
                reasons: skirmish_reasons(&mine, &foes, champions, items),
            })
        })
        .collect()
}

/// Abaixo disto a comparação de time é cedo demais para dizer alguma coisa.
const MIN_TEAM: usize = 3;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamOutlook {
    pub ours: Vec<i64>,
    pub theirs: Vec<i64>,
    /// Com os níveis e itens de agora.
    pub odds: f64,
    /// E nivelados, nos níveis 3, 6, 11 e 16.
    pub by_level: Vec<LevelOdds>,
    pub reasons: Vec<String>,
}

/// Os cinco contra os cinco.
///
/// Só compara lados do mesmo tamanho. Num draft pela metade, dizer que os seus
/// quatro ganham dos três deles é verdade trivial e leitura errada — o que
/// falta escolher é justamente o que decide.
pub fn team_outlook(
    allies: &[Lineup],
    enemies: &[Lineup],
    champions: &ChampionIndex,
    items: &ItemIndex,
) -> Option<TeamOutlook> {
    if allies.len() != enemies.len() || allies.len() < MIN_TEAM {
        return None;
    }

    let ours: Vec<Combatant> = allies.iter().map(|l| l.combatant.clone()).collect();
    let theirs: Vec<Combatant> = enemies.iter().map(|l| l.combatant.clone()).collect();

    let mut reasons = skirmish_reasons(&ours, &theirs, champions, items);

    // A mistura de dano decide o que o outro lado compra. Um time de um tipo
    // só é resolvido com um item.
    let meu_fisico = incoming_physical_share(&ours, champions, items);
    let deles_fisico = incoming_physical_share(&theirs, champions, items);

    // 80/20 e não 85/15: um time "todo AP" ainda bate de básico, e na prática
    // fica perto de 20% de dano físico em vez de zero. Exigir mais que isso
    // fazia a observação nunca aparecer.
    const TORTO: f64 = 0.80;

    if deles_fisico >= TORTO {
        reasons.push(format!(
            "O dano deles é {:.0}% físico: armadura resolve quase tudo",
            deles_fisico * 100.0
        ));
    } else if deles_fisico <= 1.0 - TORTO {
        reasons.push(format!(
            "O dano deles é {:.0}% mágico: resistência mágica resolve quase tudo",
            (1.0 - deles_fisico) * 100.0
        ));
    }

    if meu_fisico >= TORTO {
        reasons.push("O time de vocês é quase todo físico, e isso é fácil de contra-itemizar".into());
    } else if meu_fisico <= 1.0 - TORTO {
        reasons.push("O time de vocês é quase todo mágico, e isso é fácil de contra-itemizar".into());
    }

    reasons.truncate(MAX_REASONS);

    Some(TeamOutlook {
        odds: skirmish_odds(&ours, &theirs, champions, items),
        by_level: curve(&ours, &theirs, champions, items),
        ours: ours.iter().map(|c| c.champion_id).collect(),
        theirs: theirs.iter().map(|c| c.champion_id).collect(),
        reasons,
    })
}

/// Dentro desta janela o objetivo já manda na decisão de brigar.
const OBJECTIVE_WINDOW: f64 = 90.0;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub skirmish: Skirmish,
    /// Por que esta e não outra. Sem isto, mostrar uma escaramuça só parece
    /// escolha arbitrária.
    pub because: String,
}

/// Qual objetivo puxa qual agrupamento.
fn group_of(objective: &str) -> Option<&'static str> {
    match objective {
        "Dragão" | "Ancião" => Some("dragao"),
        "Barão" | "Arauto" | "Grubs" => Some("arauto"),
        _ => None,
    }
}

/// O agrupamento de duas pessoas em que você estaria.
fn my_duo(role: Role) -> Option<&'static str> {
    match role {
        Role::Top => Some("jg_top"),
        Role::Mid => Some("jg_mid"),
        Role::Bot | Role::Support => Some("bot"),
        // Jungle entra em todos, então não há um "seu" para destacar.
        Role::Jungle | Role::Unknown => None,
    }
}

fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A escaramuça que importa agora, para quando só cabe uma — o overlay.
///
/// A ordem das regras é a ordem em que um jogador pensa: primeiro o objetivo
/// que está nascendo, depois a briga da sua própria rota, e só então a mais
/// decidida do mapa.
pub fn focus(skirmishes: &[Skirmish], upcoming: &[Spawn], my_role: Role) -> Option<Focus> {
    let achar = |key: &str| skirmishes.iter().find(|s| s.key == key).cloned();

    // 1. Um objetivo prestes a nascer decide onde a briga acontece.
    let mais_proximo = upcoming
        .iter()
        .filter(|s| s.seconds_left <= OBJECTIVE_WINDOW)
        .min_by(|a, b| a.seconds_left.total_cmp(&b.seconds_left));

    if let Some(spawn) = mais_proximo {
        if let Some(skirmish) = group_of(&spawn.name).and_then(achar) {
            return Some(Focus {
                because: if spawn.seconds_left <= 0.0 {
                    format!("{} disponível", spawn.name)
                } else {
                    format!("{} em {}", spawn.name, clock(spawn.seconds_left))
                },
                skirmish,
            });
        }
    }

    // 2. Sem objetivo perto, a briga que te envolve diretamente.
    if let Some(skirmish) = my_duo(my_role).and_then(achar) {
        return Some(Focus {
            because: "a briga da sua rota".into(),
            skirmish,
        });
    }

    // 3. E se nem isso, a mais desequilibrada — é a que carrega mais
    //    informação, porque 50/50 não ajuda a decidir nada.
    skirmishes
        .iter()
        .max_by(|a, b| (a.odds - 0.5).abs().total_cmp(&(b.odds - 0.5).abs()))
        .cloned()
        .map(|skirmish| Focus {
            because: "a briga mais decidida agora".into(),
            skirmish,
        })
}

/// Quem mais pesa de cada lado, e por quê.
fn skirmish_reasons(
    ours: &[Combatant],
    theirs: &[Combatant],
    champions: &ChampionIndex,
    items: &ItemIndex,
) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(id) = heaviest(ours, theirs, champions, items) {
        out.push(format!("Quem mais pesa do seu lado: {}", name(id, champions)));
    }
    if let Some(id) = heaviest(theirs, ours, champions, items) {
        out.push(format!("Do lado deles: {}", name(id, champions)));
    }

    let my_front = ours
        .iter()
        .filter_map(|c| champions.get(c.champion_id))
        .any(|c| c.is_frontline());
    let their_front = theirs
        .iter()
        .filter_map(|c| champions.get(c.champion_id))
        .any(|c| c.is_frontline());

    if their_front && !my_front {
        out.push("Eles têm frontline no grupo e vocês não".into());
    } else if my_front && !their_front {
        out.push("Vocês têm frontline e eles não".into());
    }

    out.truncate(MAX_REASONS);
    out
}

fn heaviest(
    side: &[Combatant],
    foes: &[Combatant],
    champions: &ChampionIndex,
    items: &ItemIndex,
) -> Option<i64> {
    side.iter()
        .map(|member| {
            let solo = side_power(std::slice::from_ref(member), foes, champions, items);
            (member.champion_id, solo)
        })
        .filter(|(_, power)| power.is_finite() && *power > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::super::champion::fake;
    use super::super::items::{parse_stats, Item};
    use super::super::stats::{NoStats, Record};
    use super::*;
    use std::collections::HashMap;

    fn champions() -> ChampionIndex {
        let mut by_id = HashMap::new();
        // Mago à distância, frágil.
        by_id.insert(1, fake(1, "Ahri", "Mage", 3.0, 8.0, 4.0));
        // Assassino corpo a corpo.
        by_id.insert(2, fake(2, "Zed", "Assassin", 9.0, 3.0, 6.0));
        // Tanque.
        by_id.insert(3, fake(3, "Malphite", "Tank", 5.0, 7.0, 9.0));
        // Lutador de selva.
        by_id.insert(4, fake(4, "LeeSin", "Fighter", 8.0, 3.0, 5.0));
        by_id.insert(5, fake(5, "Viego", "Fighter", 8.0, 3.0, 5.0));
        by_id.insert(6, fake(6, "Garen", "Fighter", 8.0, 3.0, 7.0));
        by_id.insert(7, fake(7, "Ashe", "Marksman", 9.0, 2.0, 3.0));
        by_id.insert(8, fake(8, "Caitlyn", "Marksman", 9.0, 2.0, 3.0));
        by_id.insert(9, fake(9, "Thresh", "Support", 5.0, 6.0, 6.0));
        by_id.insert(10, fake(10, "Lulu", "Support", 4.0, 7.0, 4.0));

        ChampionIndex {
            version: "test".into(),
            by_id,
        }
    }

    fn items() -> ItemIndex {
        let mut by_id = HashMap::new();
        by_id.insert(
            3089,
            Item {
                id: 3089,
                name: "Chapéu de Rabadon".into(),
                cost: 3600.0,
                purchasable: true,
                stats: parse_stats("<stats><attention>130</attention> Ability Power</stats>"),
            },
        );
        by_id.insert(
            3075,
            Item {
                id: 3075,
                name: "Malha de Espinhos".into(),
                cost: 2700.0,
                purchasable: true,
                stats: parse_stats(
                    "<stats><attention>70</attention> Armor<br><attention>350</attention> Health</stats>",
                ),
            },
        );
        ItemIndex {
            version: "test".into(),
            by_id,
        }
    }

    fn lineup(role: Role, id: i64) -> Lineup {
        Lineup::new(role, id, 11, vec![])
    }

    struct FakeMatchup(i64, i64, Record);

    impl StatsStore for FakeMatchup {
        fn base(&self, _c: i64, _r: Role) -> Option<Record> {
            None
        }
        fn matchup(&self, champion: i64, opponent: i64, _role: Role) -> Option<Record> {
            (champion == self.0 && opponent == self.1).then_some(self.2)
        }
        fn synergy(&self, _c: i64, _a: i64) -> Option<Record> {
            None
        }
        fn personal(&self, _c: i64) -> Option<Record> {
            None
        }
        fn has_global(&self) -> bool {
            true
        }
    }

    #[test]
    fn so_entram_rotas_com_os_dois_lados_conhecidos() {
        let mine = vec![lineup(Role::Mid, 1), lineup(Role::Top, 6)];
        let theirs = vec![lineup(Role::Mid, 2)];

        let out = lane_outlooks(&mine, &theirs, &champions(), &items(), &NoStats);

        assert_eq!(out.len(), 1, "a rota sem oponente não pode virar 50%");
        assert_eq!(out[0].role, Role::Mid);
    }

    #[test]
    fn sem_base_de_partidas_o_numero_vem_so_do_duelo() {
        let mine = vec![lineup(Role::Mid, 1)];
        let theirs = vec![lineup(Role::Mid, 2)];

        let out = lane_outlooks(&mine, &theirs, &champions(), &items(), &NoStats);

        assert!(!out[0].from_data);
        assert_eq!(out[0].sample, 0);
        // Nenhuma razão pode alegar partidas que não existem.
        assert!(
            !out[0].reasons.iter().any(|r| r.contains("n=")),
            "{:?}",
            out[0].reasons
        );
    }

    #[test]
    fn matchup_com_amostra_empurra_a_probabilidade() {
        let mine = vec![lineup(Role::Mid, 1)];
        let theirs = vec![lineup(Role::Mid, 2)];
        let idx = champions();
        let it = items();

        let seco = lane_outlooks(&mine, &theirs, &idx, &it, &NoStats);
        let store = FakeMatchup(1, 2, Record::new(4000, 2400)); // 60%
        let com = lane_outlooks(&mine, &theirs, &idx, &it, &store);

        assert!(com[0].win_probability > seco[0].win_probability);
        assert!(com[0].from_data);
        assert_eq!(com[0].sample, 4000);
        assert!(com[0].reasons[0].contains("n=4000"), "{:?}", com[0].reasons);
    }

    #[test]
    fn itens_do_inimigo_derrubam_a_sua_rota() {
        let idx = champions();
        let it = items();
        let mine = vec![lineup(Role::Mid, 1)];

        let nu = lane_outlooks(&mine, &[lineup(Role::Mid, 2)], &idx, &it, &NoStats);
        let armado = lane_outlooks(
            &mine,
            &[Lineup::new(Role::Mid, 2, 11, vec![3075])],
            &idx,
            &it,
            &NoStats,
        );

        assert!(
            armado[0].win_probability < nu[0].win_probability,
            "{} vs {}",
            armado[0].win_probability,
            nu[0].win_probability
        );
    }

    #[test]
    fn seus_itens_sobem_a_sua_rota() {
        let idx = champions();
        let it = items();
        let theirs = vec![lineup(Role::Mid, 2)];

        let nu = lane_outlooks(&[lineup(Role::Mid, 1)], &theirs, &idx, &it, &NoStats);
        let armado = lane_outlooks(
            &[Lineup::new(Role::Mid, 1, 11, vec![3089])],
            &theirs,
            &idx,
            &it,
            &NoStats,
        );

        assert!(armado[0].win_probability > nu[0].win_probability);
    }

    #[test]
    fn a_probabilidade_nunca_chega_ao_extremo() {
        // Tanque nível 18 com item contra marksman nível 1 sem nada.
        let mine = vec![Lineup::new(Role::Top, 3, 18, vec![3075])];
        let theirs = vec![Lineup::new(Role::Top, 7, 1, vec![])];

        let out = lane_outlooks(&mine, &theirs, &champions(), &items(), &NoStats);

        assert!(out[0].win_probability <= CEILING + 1e-9);
        assert!(out[0].win_probability >= FLOOR - 1e-9);
    }

    #[test]
    fn a_rota_e_simetrica() {
        let idx = champions();
        let it = items();
        let a = lane_outlooks(&[lineup(Role::Mid, 1)], &[lineup(Role::Mid, 2)], &idx, &it, &NoStats);
        let b = lane_outlooks(&[lineup(Role::Mid, 2)], &[lineup(Role::Mid, 1)], &idx, &it, &NoStats);

        assert!((a[0].win_probability + b[0].win_probability - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_ordem_sai_sempre_top_jg_mid_adc_sup() {
        let roles = [Role::Support, Role::Top, Role::Bot, Role::Mid, Role::Jungle];
        let ids = [9, 6, 7, 1, 4];
        let mine: Vec<Lineup> = roles
            .iter()
            .zip(ids)
            .map(|(&r, id)| lineup(r, id))
            .collect();
        let theirs: Vec<Lineup> = roles
            .iter()
            .zip([10, 3, 8, 2, 5])
            .map(|(&r, id)| lineup(r, id))
            .collect();

        let out = lane_outlooks(&mine, &theirs, &champions(), &items(), &NoStats);
        let saiu: Vec<Role> = out.iter().map(|l| l.role).collect();

        assert_eq!(
            saiu,
            vec![Role::Top, Role::Jungle, Role::Mid, Role::Bot, Role::Support]
        );
    }

    fn time_completo() -> (Vec<Lineup>, Vec<Lineup>) {
        (
            vec![
                lineup(Role::Top, 6),
                lineup(Role::Jungle, 4),
                lineup(Role::Mid, 1),
                lineup(Role::Bot, 7),
                lineup(Role::Support, 9),
            ],
            vec![
                lineup(Role::Top, 3),
                lineup(Role::Jungle, 5),
                lineup(Role::Mid, 2),
                lineup(Role::Bot, 8),
                lineup(Role::Support, 10),
            ],
        )
    }

    #[test]
    fn sai_um_grupo_para_cada_briga_que_acontece() {
        let (mine, theirs) = time_completo();
        let out = skirmishes(&mine, &theirs, &champions(), &items());

        assert_eq!(out.len(), GROUPS.len());
        assert!(out.iter().any(|s| s.label == "JG+TOP x JG+TOP"));
        assert!(out.iter().any(|s| s.label == "JG+ADC+SUP x JG+ADC+SUP"));
        assert!(out.iter().all(|s| s.ours.len() == s.size));
    }

    #[test]
    fn grupo_incompleto_fica_de_fora_em_vez_de_virar_2v1() {
        let (mine, mut theirs) = time_completo();
        theirs.retain(|l| l.role != Role::Jungle);

        let out = skirmishes(&mine, &theirs, &champions(), &items());

        assert!(out.iter().all(|s| !s.label.contains("JG")), "{out:?}");
        assert!(out.iter().any(|s| s.label == "ADC+SUP x ADC+SUP"));
    }

    #[test]
    fn item_no_grupo_inimigo_derruba_a_sua_chance() {
        let (mine, theirs) = time_completo();
        let idx = champions();
        let it = items();

        let antes = skirmishes(&mine, &theirs, &idx, &it);
        let mut depois_time = theirs.clone();
        depois_time[0].combatant.items = vec![3075]; // o top deles compra

        let depois = skirmishes(&mine, &depois_time, &idx, &it);

        let topo = |v: &[Skirmish]| {
            v.iter()
                .find(|s| s.label == "JG+TOP x JG+TOP")
                .unwrap()
                .odds
        };
        assert!(topo(&depois) < topo(&antes));
    }

    #[test]
    fn a_escaramuca_e_simetrica() {
        let (mine, theirs) = time_completo();
        let idx = champions();
        let it = items();

        let a = skirmishes(&mine, &theirs, &idx, &it);
        let b = skirmishes(&theirs, &mine, &idx, &it);

        for (um, outro) in a.iter().zip(b.iter()) {
            assert!(
                (um.odds + outro.odds - 1.0).abs() < 1e-9,
                "{} {} {}",
                um.label,
                um.odds,
                outro.odds
            );
        }
    }

    #[test]
    fn as_razoes_nunca_dizem_onde_alguem_esta() {
        // A política da Riot proíbe revelar posição no mapa. Nenhuma frase
        // daqui pode sugerir que sabemos onde o inimigo está.
        let (mine, theirs) = time_completo();
        let idx = champions();
        let it = items();

        let mut todas: Vec<String> = Vec::new();
        for s in skirmishes(&mine, &theirs, &idx, &it) {
            todas.extend(s.reasons);
        }
        for l in lane_outlooks(&mine, &theirs, &idx, &it, &NoStats) {
            todas.extend(l.reasons);
        }

        for frase in &todas {
            let baixa = frase.to_lowercase();
            for proibida in ["está em", "no mapa", "visto em", "posição", "caminhando"] {
                assert!(!baixa.contains(proibida), "{frase}");
            }
        }
    }

    fn spawn(nome: &str, faltam: f64) -> Spawn {
        Spawn {
            name: nome.into(),
            at: 0.0,
            seconds_left: faltam,
            observed: true,
        }
    }

    fn brigas() -> Vec<Skirmish> {
        let (mine, theirs) = time_completo();
        skirmishes(&mine, &theirs, &champions(), &items())
    }

    #[test]
    fn o_objetivo_perto_decide_qual_briga_mostrar() {
        let b = brigas();

        let dragao = focus(&b, &[spawn("Dragão", 40.0)], Role::Top).unwrap();
        assert_eq!(dragao.skirmish.key, "dragao");
        assert_eq!(dragao.because, "Dragão em 0:40");

        let barao = focus(&b, &[spawn("Barão", 25.0)], Role::Bot).unwrap();
        assert_eq!(barao.skirmish.key, "arauto", "barão puxa o grupo de cima");
    }

    #[test]
    fn objetivo_longe_nao_manda_na_escolha() {
        let b = brigas();
        // Fora da janela: quem decide é a rota do jogador.
        let f = focus(&b, &[spawn("Dragão", 400.0)], Role::Top).unwrap();
        assert_eq!(f.skirmish.key, "jg_top");
        assert_eq!(f.because, "a briga da sua rota");
    }

    #[test]
    fn o_mais_proximo_ganha_quando_ha_dois() {
        let b = brigas();
        let f = focus(
            &b,
            &[spawn("Barão", 80.0), spawn("Dragão", 15.0)],
            Role::Mid,
        )
        .unwrap();
        assert_eq!(f.skirmish.key, "dragao");
    }

    #[test]
    fn cada_rota_puxa_a_propria_dupla() {
        let b = brigas();
        let de = |role| focus(&b, &[], role).unwrap().skirmish.key;

        assert_eq!(de(Role::Top), "jg_top");
        assert_eq!(de(Role::Mid), "jg_mid");
        assert_eq!(de(Role::Bot), "bot");
        assert_eq!(de(Role::Support), "bot");
    }

    #[test]
    fn o_jungler_cai_na_briga_mais_decidida() {
        // O jungle entra em todos os grupos, então não existe "a dele".
        let b = brigas();
        let f = focus(&b, &[], Role::Jungle).unwrap();

        assert_eq!(f.because, "a briga mais decidida agora");
        let maior = b
            .iter()
            .map(|s| (s.odds - 0.5).abs())
            .fold(0.0_f64, f64::max);
        assert!(((f.skirmish.odds - 0.5).abs() - maior).abs() < 1e-9);
    }

    #[test]
    fn objetivo_ja_disponivel_muda_o_texto() {
        let b = brigas();
        let f = focus(&b, &[spawn("Dragão", 0.0)], Role::Top).unwrap();
        assert_eq!(f.because, "Dragão disponível");
    }

    #[test]
    fn sem_escaramuca_nenhuma_nao_ha_destaque() {
        assert!(focus(&[], &[spawn("Dragão", 10.0)], Role::Mid).is_none());
    }

    #[test]
    fn objetivo_perto_sem_grupo_completo_cai_para_a_regra_seguinte() {
        // Grupo do dragão precisa de JG+ADC+SUP dos dois lados. Sem o jungle
        // deles, ele não existe — e o destaque não pode sumir por isso.
        let (mine, mut theirs) = time_completo();
        theirs.retain(|l| l.role != Role::Jungle);
        let b = skirmishes(&mine, &theirs, &champions(), &items());

        let f = focus(&b, &[spawn("Dragão", 20.0)], Role::Bot).unwrap();
        assert_eq!(f.skirmish.key, "bot");
    }

    #[test]
    fn a_curva_cobre_os_quatro_niveis() {
        let out = lane_outlooks(
            &[lineup(Role::Mid, 1)],
            &[lineup(Role::Mid, 2)],
            &champions(),
            &items(),
            &NoStats,
        );

        let niveis: Vec<u8> = out[0].by_level.iter().map(|p| p.level).collect();
        assert_eq!(niveis, BREAKPOINTS.to_vec());
        assert!(out[0].by_level.iter().all(|p| (0.0..=1.0).contains(&p.odds)));
    }

    #[test]
    fn a_curva_nivela_os_dois_lados() {
        // Durante a partida os níveis reais diferem. A curva responde outra
        // pergunta: em igualdade, de quem é o jogo. Então o nível de entrada
        // não pode influenciar o resultado.
        let idx = champions();
        let it = items();

        let desigual = lane_outlooks(
            &[Lineup::new(Role::Mid, 1, 18, vec![])],
            &[Lineup::new(Role::Mid, 2, 1, vec![])],
            &idx,
            &it,
            &NoStats,
        );
        let igual = lane_outlooks(
            &[Lineup::new(Role::Mid, 1, 7, vec![])],
            &[Lineup::new(Role::Mid, 2, 7, vec![])],
            &idx,
            &it,
            &NoStats,
        );

        assert_eq!(desigual[0].by_level, igual[0].by_level);
        assert_ne!(
            desigual[0].win_probability, igual[0].win_probability,
            "mas a chance de agora continua olhando os níveis reais"
        );
    }

    #[test]
    fn crescimento_diferente_move_a_curva() {
        // Um campeão que cresce mais por nível tem de aparecer melhor no 16
        // do que no 3 — é exatamente isto que a curva existe para mostrar.
        let mut idx = champions();
        let mut tardio = fake(50, "Tardio", "Mage", 3.0, 8.0, 4.0);
        tardio.base.health_per_level = 160.0;
        tardio.base.attack_damage_per_level = 6.0;
        idx.by_id.insert(50, tardio);

        let out = lane_outlooks(
            &[lineup(Role::Mid, 50)],
            &[lineup(Role::Mid, 2)],
            &idx,
            &items(),
            &NoStats,
        );
        let curva = &out[0].by_level;

        assert!(
            curva.last().unwrap().odds > curva[0].odds,
            "{:?}",
            curva
        );
    }

    #[test]
    fn a_curva_da_escaramuca_tambem_sai() {
        let (mine, theirs) = time_completo();
        for s in skirmishes(&mine, &theirs, &champions(), &items()) {
            assert_eq!(s.by_level.len(), BREAKPOINTS.len(), "{}", s.label);
        }
    }

    #[test]
    fn a_curva_tambem_e_simetrica() {
        let idx = champions();
        let it = items();
        let a = lane_outlooks(&[lineup(Role::Mid, 1)], &[lineup(Role::Mid, 2)], &idx, &it, &NoStats);
        let b = lane_outlooks(&[lineup(Role::Mid, 2)], &[lineup(Role::Mid, 1)], &idx, &it, &NoStats);

        for (x, y) in a[0].by_level.iter().zip(b[0].by_level.iter()) {
            assert_eq!(x.level, y.level);
            assert!((x.odds + y.odds - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn os_itens_continuam_valendo_na_curva() {
        let idx = champions();
        let it = items();
        let theirs = vec![lineup(Role::Mid, 2)];

        let nu = lane_outlooks(&[lineup(Role::Mid, 1)], &theirs, &idx, &it, &NoStats);
        let armado = lane_outlooks(
            &[Lineup::new(Role::Mid, 1, 11, vec![3089])],
            &theirs,
            &idx,
            &it,
            &NoStats,
        );

        // Nivelar o nível não pode apagar o item que a pessoa comprou.
        for (a, b) in armado[0].by_level.iter().zip(nu[0].by_level.iter()) {
            assert!(a.odds > b.odds, "nível {}", a.level);
        }
    }

    #[test]
    fn as_fases_tem_nome_e_cobrem_toda_a_faixa() {
        assert_eq!(phase_label(3), "early game");
        assert_eq!(phase_label(6), "mid game");
        assert_eq!(phase_label(11), "late game");
        assert_eq!(phase_label(18), "late game");
        assert_eq!(phase_label(1), "early game");

        // Cada ponto da curva sai com a fase dele junto.
        let out = lane_outlooks(
            &[lineup(Role::Mid, 1)],
            &[lineup(Role::Mid, 2)],
            &champions(),
            &items(),
            &NoStats,
        );
        let fases: Vec<&str> = out[0].by_level.iter().map(|p| p.phase).collect();
        assert_eq!(fases, vec!["early game", "mid game", "late game"]);
    }

    #[test]
    fn o_time_completo_rende_um_score_geral() {
        let (mine, theirs) = time_completo();
        let t = team_outlook(&mine, &theirs, &champions(), &items()).unwrap();

        assert_eq!(t.ours.len(), 5);
        assert_eq!(t.theirs.len(), 5);
        assert!((0.0..=1.0).contains(&t.odds));
        assert_eq!(t.by_level.len(), BREAKPOINTS.len());
    }

    #[test]
    fn lados_de_tamanhos_diferentes_nao_viram_score() {
        // Quatro contra três não é leitura: o que falta escolher é o que
        // decide, e "seus quatro ganham dos três deles" é verdade trivial.
        let (mine, mut theirs) = time_completo();
        theirs.pop();

        assert!(team_outlook(&mine, &theirs, &champions(), &items()).is_none());
    }

    #[test]
    fn draft_no_comeco_e_cedo_demais_para_score_de_time() {
        let (mine, theirs) = time_completo();
        assert!(team_outlook(&mine[..2], &theirs[..2], &champions(), &items()).is_none());
        assert!(team_outlook(&mine[..3], &theirs[..3], &champions(), &items()).is_some());
    }

    #[test]
    fn o_score_de_time_e_simetrico() {
        let (mine, theirs) = time_completo();
        let idx = champions();
        let it = items();

        let a = team_outlook(&mine, &theirs, &idx, &it).unwrap();
        let b = team_outlook(&theirs, &mine, &idx, &it).unwrap();

        assert!((a.odds + b.odds - 1.0).abs() < 1e-9);
        for (x, y) in a.by_level.iter().zip(b.by_level.iter()) {
            assert!((x.odds + y.odds - 1.0).abs() < 1e-9, "nível {}", x.level);
        }
    }

    #[test]
    fn time_de_um_tipo_de_dano_so_e_apontado() {
        // Cinco magos contra cinco magos: os dois lados são fáceis de
        // contra-itemizar, e isso é informação de draft.
        let magos: Vec<Lineup> = [Role::Top, Role::Jungle, Role::Mid, Role::Bot, Role::Support]
            .iter()
            .map(|&r| lineup(r, 1))
            .collect();
        let outros: Vec<Lineup> = [Role::Top, Role::Jungle, Role::Mid, Role::Bot, Role::Support]
            .iter()
            .map(|&r| lineup(r, 2))
            .collect();

        let t = team_outlook(&magos, &outros, &champions(), &items()).unwrap();
        assert!(
            t.reasons.iter().any(|r| r.contains("mágico") || r.contains("físico")),
            "{:?}",
            t.reasons
        );
    }

    #[test]
    fn campeao_desconhecido_nao_derruba_a_analise() {
        let mine = vec![lineup(Role::Mid, 999)];
        let theirs = vec![lineup(Role::Mid, 2)];

        let out = lane_outlooks(&mine, &theirs, &champions(), &items(), &NoStats);
        assert_eq!(out.len(), 1);
        assert!(out[0].win_probability.is_finite());
    }
}
