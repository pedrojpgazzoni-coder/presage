//! A win condition da partida: por onde este jogo é ganho.
//!
//! Não é mais um número. É uma leitura em três partes — **quando** vocês são
//! mais fortes, **o que fazer** nessa janela e **o que evitar** — montada a
//! partir do que os outros módulos já calcularam: a curva de fases do 5v5, as
//! rotas e as escaramuças.
//!
//! Tudo aqui é derivado. Este módulo não inventa número nenhum: se a curva do
//! time não existe (draft pela metade), ele não responde, porque uma win
//! condition dita com três picks na tela é chute com cara de plano.

use serde::Serialize;

use super::champion::{Champion, ChampionIndex};
use super::lanes::{LaneOutlook, Skirmish, TeamOutlook};

/// Abaixo desta diferença entre a melhor e a pior fase, o jogo não tem janela:
/// são 2 pontos percentuais, dentro do erro do próprio modelo.
const MIN_SPREAD: f64 = 0.02;

/// Diferença a partir da qual a janela é tão marcada quanto este modelo
/// consegue afirmar. Serve só para normalizar a confiança entre 0 e 1.
const SPREAD_FORTE: f64 = 0.06;

/// Uma rota só vira plano a partir daqui.
const ROTA_BOA: f64 = 0.54;
/// E vira problema a partir daqui.
const ROTA_RUIM: f64 = 0.46;

const MAX_LINHAS: usize = 3;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WinCondition {
    /// Uma frase. É o que a pessoa lê se ler só uma coisa.
    pub headline: String,
    /// `early game`, `mid game` ou `late game`.
    pub window: &'static str,
    /// Quão marcada é a janela, de 0 a 1. Perto de zero quer dizer
    /// "equilibrado", não "não sei".
    pub confidence: f64,
    /// O que fazer dentro da janela.
    pub play: Vec<String>,
    /// O que entrega o jogo.
    pub avoid: Vec<String>,
    /// Falso enquanto nada disto vier de partidas reais. A interface usa isto
    /// para não chamar de estatística o que é modelo.
    pub from_data: bool,
}

fn perfil(ids: &[i64], champions: &ChampionIndex) -> (usize, usize) {
    let times: Vec<&Champion> = ids.iter().filter_map(|id| champions.get(*id)).collect();
    (
        times.iter().filter(|c| c.is_frontline()).count(),
        times.iter().filter(|c| c.is_ranged()).count(),
    )
}

fn pct(odds: f64) -> String {
    format!("{:.0}%", odds * 100.0)
}

/// A leitura do jogo, ou nada.
///
/// `team` vem de [`super::lanes::team_outlook`] e é obrigatório: é dele que sai
/// a curva de fases. Sem ela não há janela, e sem janela não há win condition.
pub fn win_condition(
    team: &TeamOutlook,
    lanes: &[LaneOutlook],
    skirmishes: &[Skirmish],
    champions: &ChampionIndex,
) -> Option<WinCondition> {
    if team.by_level.is_empty() {
        return None;
    }

    let melhor = team
        .by_level
        .iter()
        .max_by(|a, b| a.odds.total_cmp(&b.odds))?;
    let pior = team
        .by_level
        .iter()
        .min_by(|a, b| a.odds.total_cmp(&b.odds))?;

    let spread = melhor.odds - pior.odds;
    let equilibrado = spread < MIN_SPREAD;
    let confidence = (spread / SPREAD_FORTE).min(1.0);

    let headline = if equilibrado {
        "Jogo parelho nas três fases: quem errar menos leva".to_string()
    } else {
        format!(
            "Vocês são mais fortes no {}: {} lá, {} no {}",
            melhor.phase,
            pct(melhor.odds),
            pct(pior.odds),
            pior.phase
        )
    };

    let mut play = Vec::new();
    let mut avoid = Vec::new();

    if !equilibrado {
        play.push(match melhor.phase {
            "early game" => {
                "Briguem cedo. Invadam, puxem gank e troquem torre por torre antes dos 15 minutos"
                    .to_string()
            }
            "mid game" => {
                "Agrupem em volta do primeiro arauto e dos dragões: é ali que a diferença é maior"
                    .to_string()
            }
            _ => "Joguem para o tempo. Farmem seguro, dividam o mapa e só briguem com visão"
                .to_string(),
        });

        avoid.push(match pior.phase {
            "early game" => "Não aceitem luta antes do nível 6 — vocês perdem essa fase".to_string(),
            "mid game" => {
                "Evitem briga arrumada no meio do jogo; segurem até os itens completos".to_string()
            }
            _ => "Não deixem o jogo passar dos 30 minutos: a vantagem de vocês some".to_string(),
        });
    }

    // A rota mais favorável é o eixo concreto do plano: é onde o jogo tem mais
    // chance de abrir. A pior é por onde ele é perdido.
    let mut ordenadas: Vec<&LaneOutlook> = lanes.iter().collect();
    ordenadas.sort_by(|a, b| b.win_probability.total_cmp(&a.win_probability));

    if let Some(top) = ordenadas.first() {
        if top.win_probability >= ROTA_BOA {
            play.push(format!(
                "Joguem pela {}: {} tem {} contra {}",
                top.lane,
                top.ally_name,
                pct(top.win_probability),
                top.enemy_name
            ));
        }
    }

    if let Some(fundo) = ordenadas.last() {
        if fundo.win_probability <= ROTA_RUIM {
            avoid.push(format!(
                "Não deixem a {} sozinha: {} tem {} contra {}",
                fundo.lane,
                fundo.ally_name,
                pct(fundo.win_probability),
                fundo.enemy_name
            ));
        }
    }

    // A escaramuça mais favorável diz com quem agrupar, que é a pergunta que
    // a pessoa realmente faz no minuto 8.
    if let Some(melhor_luta) = skirmishes.iter().max_by(|a, b| a.odds.total_cmp(&b.odds)) {
        if melhor_luta.odds >= ROTA_BOA {
            play.push(format!(
                "{} é a sua melhor luta ({}): procurem {}",
                melhor_luta.label,
                pct(melhor_luta.odds),
                melhor_luta.detail
            ));
        }
    }

    let (nossa_frente, nossos_longe) = perfil(&team.ours, champions);
    let (frente_deles, _) = perfil(&team.theirs, champions);

    // Comp sem quem inicie não escolhe a luta: ela espera. Dizer isso é mais
    // útil que repetir a porcentagem.
    if nossa_frente == 0 {
        avoid.push("Ninguém no time de vocês inicia: não procurem luta aberta, esperem o erro deles".into());
    } else if nossos_longe >= 3 && frente_deles <= 1 {
        play.push("Vocês têm alcance e eles não têm quem entre: pressionem de longe e neguem o agrupamento".into());
    }

    play.truncate(MAX_LINHAS);
    avoid.truncate(MAX_LINHAS);

    Some(WinCondition {
        headline,
        window: melhor.phase,
        confidence,
        play,
        avoid,
        // Nada aqui sai de partidas agregadas ainda. Quando a base existir, é
        // este campo que a interface olha para mudar o texto.
        from_data: false,
    })
}

#[cfg(test)]
mod tests {
    use super::super::champion::fake;
    use super::super::lanes::LevelOdds;
    use super::*;
    use std::collections::HashMap;

    fn index() -> ChampionIndex {
        ChampionIndex {
            version: "test".into(),
            by_id: vec![
                fake(1, "Tanque", "Tank", 5.0, 6.0, 9.0),
                fake(2, "Marks", "Marksman", 9.0, 2.0, 3.0),
                fake(3, "Mago", "Mage", 2.0, 9.0, 4.0),
            ]
            .into_iter()
            .map(|c| (c.id, c))
            .collect::<HashMap<_, _>>(),
        }
    }

    fn curva(early: f64, mid: f64, late: f64) -> Vec<LevelOdds> {
        vec![
            LevelOdds { level: 3, phase: "early game", odds: early },
            LevelOdds { level: 6, phase: "mid game", odds: mid },
            LevelOdds { level: 11, phase: "late game", odds: late },
        ]
    }

    fn time(curva: Vec<LevelOdds>, ours: Vec<i64>, theirs: Vec<i64>) -> TeamOutlook {
        TeamOutlook {
            odds: 0.5,
            by_level: curva,
            ours,
            theirs,
            reasons: vec![],
        }
    }

    #[test]
    fn janela_e_a_fase_mais_forte() {
        let t = time(curva(0.58, 0.50, 0.44), vec![1, 2, 3], vec![1, 2, 3]);
        let wc = win_condition(&t, &[], &[], &index()).unwrap();
        assert_eq!(wc.window, "early game");
        assert!(wc.headline.contains("early game"));
        assert!(!wc.play.is_empty());
        assert!(!wc.avoid.is_empty());
    }

    #[test]
    fn late_tambem_vira_plano() {
        let t = time(curva(0.44, 0.49, 0.57), vec![1, 2, 3], vec![1, 2, 3]);
        let wc = win_condition(&t, &[], &[], &index()).unwrap();
        assert_eq!(wc.window, "late game");
        assert!(wc.play[0].contains("tempo"));
    }

    #[test]
    fn curva_plana_nao_inventa_janela() {
        let t = time(curva(0.502, 0.500, 0.498), vec![1, 2, 3], vec![1, 2, 3]);
        let wc = win_condition(&t, &[], &[], &index()).unwrap();
        assert!(wc.headline.contains("parelho"));
        assert!(wc.confidence < 0.4);
    }

    #[test]
    fn confianca_cresce_com_a_diferenca() {
        let idx = index();
        let fraca = win_condition(
            &time(curva(0.52, 0.50, 0.49), vec![1], vec![1]),
            &[],
            &[],
            &idx,
        )
        .unwrap();
        let forte = win_condition(
            &time(curva(0.60, 0.50, 0.42), vec![1], vec![1]),
            &[],
            &[],
            &idx,
        )
        .unwrap();
        assert!(forte.confidence > fraca.confidence);
        assert!(forte.confidence <= 1.0);
    }

    #[test]
    fn sem_curva_nao_responde() {
        let t = time(vec![], vec![1], vec![1]);
        assert!(win_condition(&t, &[], &[], &index()).is_none());
    }

    #[test]
    fn time_sem_frontline_e_avisado() {
        // Dois à distância e um mago: ninguém inicia.
        let t = time(curva(0.50, 0.50, 0.50), vec![2, 3], vec![1, 1]);
        let wc = win_condition(&t, &[], &[], &index()).unwrap();
        assert!(wc.avoid.iter().any(|linha| linha.contains("inicia")));
    }

    #[test]
    fn nada_sai_de_base_ainda() {
        let t = time(curva(0.58, 0.50, 0.44), vec![1], vec![1]);
        assert!(!win_condition(&t, &[], &[], &index()).unwrap().from_data);
    }
}
