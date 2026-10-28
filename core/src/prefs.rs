//! O que o jogador quer ver por cima do jogo.
//!
//! O overlay não tem uma resposta certa. Quem joga de suporte olha objetivo e
//! ignora build; quem joga de ADC quer o próximo item e acha o ouro do time
//! ruído. Decidir por todo mundo seria errar com a maioria.
//!
//! Então cada seção liga e desliga, e a escolha mora aqui — num arquivo, para
//! sobreviver a fechar o app.

use serde::{Deserialize, Serialize};

/// Uma seção do overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    Gold,
    Damage,
    Objectives,
    Priority,
    Skirmish,
    NextItem,
}

impl Section {
    /// Todas, na ordem em que aparecem no overlay.
    pub const ALL: [Section; 6] = [
        Section::Gold,
        Section::Damage,
        Section::Objectives,
        Section::Priority,
        Section::Skirmish,
        Section::NextItem,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Gold => "Ouro em itens",
            Section::Damage => "Defesa contra o dano deles",
            Section::Objectives => "Objetivos",
            Section::Priority => "Prioridade de rota",
            Section::Skirmish => "Escaramuça",
            Section::NextItem => "Próximo item",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Section::Gold => "a diferença entre os times, soma exata dos inventários",
            Section::Damage => {
                "quanto do dano deles é físico, sua vida efetiva e a penetração deles"
            }
            Section::Objectives => "dragão, barão, arauto e grubs",
            Section::Priority => "qual rota vale mais agora, com o motivo",
            Section::Skirmish => "a briga que importa neste minuto",
            Section::NextItem => "duas opções, contra o que eles têm agora",
        }
    }
}

/// Onde ficam os relógios de objetivo, em cima do minimapa do jogo.
///
/// O tamanho do minimapa é escolhido por quem joga, nas opções do League, e
/// **não existe jeito de ler isso de fora** — a Live Client Data API não diz, e
/// ler a memória do jogo está fora de questão. Então o número mora aqui e quem
/// ajusta é o jogador, uma vez: ele arrasta até os relógios caírem em cima das
/// covas do Barão e do Dragão, e pronto.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MapTimers {
    /// Lado do quadrado do minimapa, em pixels.
    pub size: u32,
    /// Distância da borda direita da tela.
    pub right: u32,
    /// Distância da borda de baixo.
    pub bottom: u32,
}

/// Medidas de um minimapa em tamanho médio a 1920x1080. É daqui que o jogador
/// parte, não de zero.
impl Default for MapTimers {
    fn default() -> Self {
        MapTimers {
            size: 330,
            right: 14,
            bottom: 14,
        }
    }
}

impl MapTimers {
    /// Limites de arrasto. Um minimapa menor que isso não existe, e maior que
    /// isso cobriria meia tela — deixar o slider ir além só gera configuração
    /// quebrada.
    pub const MIN_SIZE: u32 = 180;
    pub const MAX_SIZE: u32 = 560;
    pub const MAX_OFFSET: u32 = 400;

    /// Recorta valores impossíveis em vez de recusá-los: um arquivo de
    /// configuração velho, ou editado à mão, continua abrindo o app.
    pub fn clamped(self) -> Self {
        MapTimers {
            size: self.size.clamp(Self::MIN_SIZE, Self::MAX_SIZE),
            right: self.right.min(Self::MAX_OFFSET),
            bottom: self.bottom.min(Self::MAX_OFFSET),
        }
    }
}

/// Seção escondida pelo jogador.
///
/// Guardamos o que está **desligado**, não o que está ligado. A diferença
/// aparece quando uma seção nova chega: com a lista de ligados, ela nasceria
/// invisível para todo mundo que já tem configuração salva, e ninguém
/// descobriria que ela existe. Guardando os desligados, o que é novo aparece.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct OverlayPrefs {
    pub hidden: Vec<Section>,
    /// Onde os relógios caem em cima do minimapa.
    pub map: MapTimers,
}

impl OverlayPrefs {
    pub fn shows(&self, section: Section) -> bool {
        !self.hidden.contains(&section)
    }

    pub fn set(&mut self, section: Section, visible: bool) {
        self.hidden.retain(|s| *s != section);
        if !visible {
            self.hidden.push(section);
        }
    }

    /// Tudo desligado quer dizer overlay vazio, e overlay vazio é uma janela
    /// invisível ocupando memória. Quem chama decide o que fazer.
    pub fn anything_visible(&self) -> bool {
        Section::ALL.iter().any(|s| self.shows(*s))
    }

    pub fn visible(&self) -> Vec<Section> {
        Section::ALL.iter().copied().filter(|s| self.shows(*s)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn por_padrao_tudo_aparece() {
        let p = OverlayPrefs::default();
        for s in Section::ALL {
            assert!(p.shows(s), "{s:?} deveria aparecer");
        }
        assert_eq!(p.visible().len(), Section::ALL.len());
    }

    #[test]
    fn desligar_e_religar_volta_ao_estado_anterior() {
        let mut p = OverlayPrefs::default();
        p.set(Section::Gold, false);
        assert!(!p.shows(Section::Gold));
        assert!(p.shows(Section::Damage), "não pode mexer nas outras");

        p.set(Section::Gold, true);
        assert_eq!(p, OverlayPrefs::default());
    }

    #[test]
    fn desligar_duas_vezes_nao_duplica() {
        let mut p = OverlayPrefs::default();
        p.set(Section::Skirmish, false);
        p.set(Section::Skirmish, false);
        assert_eq!(p.hidden.len(), 1);
    }

    #[test]
    fn tudo_desligado_e_detectavel() {
        let mut p = OverlayPrefs::default();
        assert!(p.anything_visible());
        for s in Section::ALL {
            p.set(s, false);
        }
        assert!(!p.anything_visible());
        assert!(p.visible().is_empty());
    }

    #[test]
    fn a_ordem_das_visiveis_e_a_do_overlay() {
        let mut p = OverlayPrefs::default();
        p.set(Section::Objectives, false);
        assert_eq!(
            p.visible(),
            vec![
                Section::Gold,
                Section::Damage,
                Section::Priority,
                Section::Skirmish,
                Section::NextItem,
            ]
        );
    }

    #[test]
    fn secao_nova_nasce_visivel_para_quem_ja_tinha_configuracao() {
        // O motivo de guardar os desligados. Este arquivo foi salvo antes de
        // `skirmish` existir, e ela tem de aparecer mesmo assim.
        let antigo = r#"{"hidden":["gold"]}"#;
        let p: OverlayPrefs = serde_json::from_str(antigo).unwrap();

        assert!(!p.shows(Section::Gold));
        assert!(p.shows(Section::Skirmish), "o que é novo tem de aparecer");
    }

    #[test]
    fn arquivo_vazio_ou_corrompido_nao_esconde_nada() {
        let vazio: OverlayPrefs = serde_json::from_str("{}").unwrap();
        assert!(vazio.anything_visible());
        assert_eq!(vazio, OverlayPrefs::default());
    }

    #[test]
    fn sobrevive_a_ida_e_volta_em_json() {
        let mut p = OverlayPrefs::default();
        p.set(Section::Gold, false);
        p.set(Section::NextItem, false);

        let texto = serde_json::to_string(&p).unwrap();
        let voltou: OverlayPrefs = serde_json::from_str(&texto).unwrap();

        assert_eq!(voltou, p);
        assert!(!voltou.shows(Section::NextItem));
        assert!(voltou.shows(Section::Damage));
    }

    #[test]
    fn o_minimapa_tem_medida_de_partida() {
        let m = MapTimers::default();
        assert_eq!(m, m.clamped(), "o padrão não pode cair fora dos limites");
        assert!(m.size >= MapTimers::MIN_SIZE && m.size <= MapTimers::MAX_SIZE);
    }

    #[test]
    fn configuracao_impossivel_e_recortada_em_vez_de_recusada() {
        let absurdo = MapTimers { size: 9000, right: 9000, bottom: 0 };
        let certo = absurdo.clamped();
        assert_eq!(certo.size, MapTimers::MAX_SIZE);
        assert_eq!(certo.right, MapTimers::MAX_OFFSET);
        assert_eq!(certo.bottom, 0, "zero é válido: minimapa colado na borda");

        let minusculo = MapTimers { size: 1, right: 0, bottom: 0 }.clamped();
        assert_eq!(minusculo.size, MapTimers::MIN_SIZE);
    }

    #[test]
    fn arquivo_antigo_ganha_o_minimapa_padrao() {
        // Salvo antes de `map` existir: tem de abrir com a medida de partida.
        let antigo = r#"{"hidden":["gold"]}"#;
        let p: OverlayPrefs = serde_json::from_str(antigo).unwrap();
        assert_eq!(p.map, MapTimers::default());
    }

    #[test]
    fn toda_secao_tem_rotulo_e_explicacao() {
        for s in Section::ALL {
            assert!(!s.label().is_empty(), "{s:?}");
            assert!(!s.detail().is_empty(), "{s:?}");
        }
    }
}
