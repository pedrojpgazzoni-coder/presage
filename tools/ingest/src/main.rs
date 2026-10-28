//! Coletor que produz a `base-de-partidas.json` que o Presage lê.
//!
//! O desenho em uma frase: semeia jogadores de um intervalo de elo pelo
//! league-v4, pede as últimas partidas ranqueadas de cada um pelo match-v5, e
//! agrega vitórias por campeão, por matchup e por dupla.
//!
//! Duas decisões que valem explicação.
//!
//! **Não há bola de neve.** Dava para pegar os dez jogadores de cada partida
//! baixada e expandir a partir deles, o que cresce muito mais rápido. Mas aí a
//! amostra escorrega para fora do elo semeado, e o cabeçalho da base passa a
//! mentir sobre de quem são aquelas partidas. Só os jogadores semeados são
//! expandidos.
//!
//! **Um patch por base.** A primeira partida aceita trava o patch e as de
//! outro são recusadas. Misturar patch é o jeito mais fácil de uma base
//! parecer grande e recomendar item que mudou de preço.
//!
//! A chave sai de `PRESAGE_RIOT_API_KEY` e de lugar nenhum mais. Nunca de
//! argumento: argumento aparece na lista de processos.

mod aggregate;
mod api;
mod limiter;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use aggregate::{coverage, Counts, Filter, Skip, USEFUL};
use api::{is_apex, tiers_from, Riot, DIVISIONS};
use limiter::Limiter;
use presage_core::engine::dataset::Dataset;
use presage_core::riot::Region;

/// Limite inicial: o da chave de desenvolvedor, que é o caso mais apertado.
/// A primeira resposta substitui isto pelo limite real da chave.
const LIMITE_PADRAO: &str = "20:1,100:120";

/// Entradas por página do league-v4, para estimar quantas páginas pedir.
const POR_PAGINA: usize = 205;

/// Grava estado e base a cada tantas partidas processadas.
///
/// O estado carrega todo id já visto, então ele cresce para dezenas de
/// megabytes num crawl grande. Salvar de 500 em 500 no ritmo da chave de
/// produção daria uma gravação a cada dez segundos — alguns megabytes por
/// segundo de disco por nada. Perder 2000 partidas numa queda é barato; o
/// coletor rebaixa as mesmas em minutos.
const SALVAR_A_CADA: u64 = 2000;

#[derive(Debug, Clone)]
struct Opcoes {
    regiao: String,
    tier: String,
    jogadores: usize,
    por_jogador: u32,
    fila: i64,
    patch: Option<String>,
    min_jogos: u32,
    paralelo: usize,
    limite: String,
    saida: PathBuf,
    estado: PathBuf,
    retomar: bool,
    plano: bool,
    verificar: Option<PathBuf>,
}

fn pasta_do_app() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(|raiz| PathBuf::from(raiz).join("Presage"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

impl Default for Opcoes {
    fn default() -> Self {
        Opcoes {
            regiao: "br1".into(),
            tier: "EMERALD".into(),
            jogadores: 20_000,
            por_jogador: 20,
            fila: 420,
            patch: None,
            min_jogos: 20,
            paralelo: 8,
            limite: LIMITE_PADRAO.into(),
            // Exatamente onde o app procura.
            saida: pasta_do_app().join("base-de-partidas.json"),
            estado: pasta_do_app().join("coleta-em-andamento.json"),
            retomar: false,
            plano: false,
            verificar: None,
        }
    }
}

const AJUDA: &str = "\
Coletor da base de partidas do Presage.

  --regiao <br1>          host de plataforma
  --tier <EMERALD>        elo mínimo; tudo acima entra junto
  --jogadores <20000>     quantos jogadores semear
  --por-jogador <20>      partidas pedidas por jogador
  --fila <420>            420 = ranqueada solo
  --patch <15.21>         trava o patch; sem isto, trava no da 1ª partida
  --min-jogos <20>        corta entradas com amostra menor que isto
  --paralelo <8>          chamadas simultâneas
  --limite <20:1,100:120> limite inicial, no formato do cabeçalho da Riot
  --saida <arquivo>       onde gravar a base
  --estado <arquivo>      onde gravar o progresso
  --retomar               continua de onde o estado parou
  --plano                 só mostra o que faria, sem rede e sem chave
  --verificar <arquivo>   relatório de cobertura de uma base já pronta

A chave vem de PRESAGE_RIOT_API_KEY.
";

fn ler_opcoes() -> Result<Opcoes> {
    let mut o = Opcoes::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        let mut valor = || -> Result<String> {
            args.next()
                .with_context(|| format!("{arg} precisa de um valor"))
        };

        match arg.as_str() {
            "--regiao" => o.regiao = valor()?,
            "--tier" => o.tier = valor()?.to_ascii_uppercase(),
            "--jogadores" => o.jogadores = valor()?.parse().context("--jogadores")?,
            "--por-jogador" => o.por_jogador = valor()?.parse().context("--por-jogador")?,
            "--fila" => o.fila = valor()?.parse().context("--fila")?,
            "--patch" => o.patch = Some(valor()?),
            "--min-jogos" => o.min_jogos = valor()?.parse().context("--min-jogos")?,
            "--paralelo" => o.paralelo = valor()?.parse::<usize>().context("--paralelo")?.max(1),
            "--limite" => o.limite = valor()?,
            "--saida" => o.saida = valor()?.into(),
            "--estado" => o.estado = valor()?.into(),
            "--retomar" => o.retomar = true,
            "--plano" => o.plano = true,
            "--verificar" => o.verificar = Some(valor()?.into()),
            "-h" | "--help" | "--ajuda" => {
                print!("{AJUDA}");
                std::process::exit(0);
            }
            outro => bail!("opção desconhecida: {outro}\n\n{AJUDA}"),
        }
    }

    Ok(o)
}

/// O que sobrevive a uma interrupção.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Estado {
    regiao: String,
    tier: String,
    fila: i64,
    /// Os puuids semeados, na ordem.
    jogadores: Vec<String>,
    /// Os tiers que de fato entregaram jogadores.
    ///
    /// Não é o mesmo que os tiers pedidos: a cota de `--jogadores` pode acabar
    /// no meio, e aí o cabeçalho não pode dizer `EMERALD+` quando a amostra é
    /// só de esmeralda.
    tiers_semeados: Vec<String>,
    /// Até onde a lista acima já foi consumida.
    proximo: usize,
    vistas: HashSet<String>,
    contagens: Counts,
}

impl Estado {
    /// Retomar com outro recorte produziria uma base que mistura dois crawls
    /// sob um cabeçalho só.
    fn combina(&self, o: &Opcoes) -> bool {
        self.regiao == o.regiao && self.tier == o.tier && self.fila == o.fila
    }
}

fn gravar<T: Serialize>(caminho: &PathBuf, valor: &T) -> Result<()> {
    if let Some(pai) = caminho.parent() {
        std::fs::create_dir_all(pai).ok();
    }
    // Grava ao lado e renomeia: um Ctrl-C no meio da escrita não pode deixar
    // um arquivo meio escrito no lugar de um que estava bom.
    let temporario = caminho.with_extension("parcial");
    std::fs::write(&temporario, serde_json::to_vec(valor)?)
        .with_context(|| format!("gravando {}", temporario.display()))?;
    std::fs::rename(&temporario, caminho)
        .with_context(|| format!("renomeando para {}", caminho.display()))?;
    Ok(())
}

/// Nenhum sinal de que o arquivo um dia foi uma base.
fn parece_vazio(dataset: &Dataset) -> bool {
    dataset.patch.is_empty()
        && dataset.matches == 0
        && dataset.base.is_empty()
        && dataset.matchup.is_empty()
        && dataset.synergy.is_empty()
        && dataset.runes.is_empty()
        && dataset.items.is_empty()
}

fn relatorio(dataset: &Dataset) {
    let cob = coverage(dataset);
    let pct = |parte: usize, todo: usize| {
        if todo == 0 {
            0.0
        } else {
            parte as f64 * 100.0 / todo as f64
        }
    };

    println!("\n  patch {} · {} · {}", dataset.patch, dataset.region, dataset.tier);
    println!("  partidas agregadas: {}", dataset.matches);
    println!(
        "  winrate base:  {:>7} entradas, {} com n>={USEFUL} ({:.0}%)",
        cob.base,
        cob.base_util,
        pct(cob.base_util, cob.base)
    );
    println!(
        "  matchup:       {:>7} entradas, {} com n>={USEFUL} ({:.0}%)",
        cob.matchup,
        cob.matchup_util,
        pct(cob.matchup_util, cob.matchup)
    );
    println!("  sinergia:      {:>7} entradas", cob.synergy);
    println!(
        "  runas:         {:>7} entradas, {} com n>={USEFUL} ({:.0}%)",
        cob.runes,
        cob.runes_util,
        pct(cob.runes_util, cob.runes)
    );
    println!(
        "  itens:         {:>7} entradas, {} com n>={USEFUL} ({:.0}%)",
        cob.items,
        cob.items_util,
        pct(cob.items_util, cob.items)
    );

    if !dataset.is_usable() {
        println!("\n  Esta base ainda não serve: o app vai ignorá-la.");
    } else if cob.matchup_util == 0 {
        println!(
            "\n  A winrate base já diz alguma coisa, mas nenhum matchup tem amostra\n  \
             suficiente para mexer no ranking. Precisa de mais partidas."
        );
    }
}

fn mostrar_plano(o: &Opcoes) -> Result<()> {
    let tiers = tiers_from(&o.tier)
        .with_context(|| format!("{} não é um tier conhecido", o.tier))?;
    let limiter = Limiter::parse(&o.limite)
        .with_context(|| format!("limite inválido: {}", o.limite))?;
    let taxa = limiter.sustained_rate();

    let semeadura = o.jogadores.div_ceil(POR_PAGINA);
    let listas = o.jogadores;
    // Cada partida é vista por até dez jogadores semeados. Quanto maior a
    // fatia do elo que a semeadura cobre, mais repetição. 0,6 é um chute
    // honesto do que sobra único.
    let unicas = (o.jogadores as f64 * o.por_jogador as f64 * 0.6) as u64;
    let total = semeadura as u64 + listas as u64 + unicas;
    let horas = total as f64 / taxa / 3600.0;

    println!("Plano\n");
    println!("  região:     {} ({})", o.regiao, Region::from_platform(&o.regiao).map(|r| r.routing).unwrap_or("?"));
    println!("  elos:       {}", tiers.join(", "));
    println!("  divisões:   {} por tier (exceto mestre+)", DIVISIONS.len());
    println!("  jogadores:  {}", o.jogadores);
    println!("  partidas:   até {} por jogador\n", o.por_jogador);
    println!(
        "  A semeadura percorre os elos de baixo para cima e para ao bater a cota.\n  \
         Um tier populoso sozinho costuma ter mais que {} jogadores, então ela\n  \
         pode parar dentro de {}. O cabeçalho da base registra só os elos que\n  \
         entraram de verdade.\n",
        o.jogadores,
        tiers.first().copied().unwrap_or(&o.tier)
    );

    println!("Chamadas estimadas\n");
    println!("  semeadura:        {semeadura:>9}");
    println!("  listas de partida:{listas:>9}");
    println!("  partidas únicas:  {unicas:>9}  (estimativa; a repetição real varia)");
    println!("  total:            {total:>9}\n");

    println!("  taxa sustentável: {taxa:.2} chamadas/s");
    println!("  tempo:            {horas:.1} horas");

    if taxa < 2.0 {
        // A chave de desenvolvedor expira a cada 24 horas, então um crawl que
        // precisa de mais que isso não termina — não é lentidão, é impossível.
        let producao = Limiter::parse("500:10,30000:600")
            .expect("spec fixa")
            .sustained_rate();
        println!(
            "\n  Esse é o ritmo da chave de desenvolvedor, e ela expira a cada 24h:\n  \
             um crawl de {horas:.0} horas não chega ao fim.\n  \
             Com a chave de produção ({producao:.0} chamadas/s) seriam {:.1} horas.",
            total as f64 / producao / 3600.0
        );
    }

    println!(
        "\nQuanto dá para esperar\n\n  \
         Com {unicas} partidas, a winrate base por rota fica sólida.\n  \
         Matchup é outra ordem de grandeza: são milhares de pares por rota, e\n  \
         para a maioria passar de n={USEFUL} são necessários alguns milhões de\n  \
         partidas. A base funciona antes disso — o motor encolhe o que tem\n  \
         amostra pequena até o ponto de não mexer no ranking."
    );

    Ok(())
}

/// Os jogadores semeados e os tiers que realmente contribuíram.
async fn semear(riot: &Riot, o: &Opcoes) -> Result<(Vec<String>, Vec<String>)> {
    let tiers = tiers_from(&o.tier)
        .with_context(|| format!("{} não é um tier conhecido", o.tier))?;
    let fila = "RANKED_SOLO_5x5";

    let mut puuids = Vec::new();
    let mut vistos = HashSet::new();
    let mut contribuiram = Vec::new();

    for tier in tiers {
        if puuids.len() >= o.jogadores {
            break;
        }
        let antes = puuids.len();

        if is_apex(tier) {
            let entradas = riot.apex_league(tier, fila).await?;
            for entrada in entradas {
                if !entrada.puuid.is_empty() && vistos.insert(entrada.puuid.clone()) {
                    puuids.push(entrada.puuid);
                }
            }
            tracing::info!(tier, total = puuids.len(), "semeado");
        } else {
            for division in DIVISIONS {
                let mut page = 1;
                loop {
                    if puuids.len() >= o.jogadores {
                        break;
                    }
                    let entradas = riot.league_entries(fila, tier, division, page).await?;
                    if entradas.is_empty() {
                        break;
                    }
                    for entrada in entradas {
                        if !entrada.puuid.is_empty() && vistos.insert(entrada.puuid.clone()) {
                            puuids.push(entrada.puuid);
                        }
                    }
                    page += 1;
                }
                tracing::info!(tier, division, total = puuids.len(), "semeado");
            }
        }

        if puuids.len() > antes {
            contribuiram.push(tier.to_string());
        }
    }

    if puuids.is_empty() {
        bail!(
            "a semeadura não devolveu nenhum jogador. \
             Confira a região ({}) e o tier ({}).",
            o.regiao,
            o.tier
        );
    }

    puuids.truncate(o.jogadores);
    Ok((puuids, contribuiram))
}

/// O rótulo de elo que descreve a amostra de verdade.
///
/// `EMERALD+` só quando a semeadura chegou mesmo até o topo. Parou antes, o
/// cabeçalho lista o que entrou.
fn rotulo_de_elo(estado: &Estado, o: &Opcoes) -> String {
    let semeados = &estado.tiers_semeados;
    if semeados.is_empty() {
        return o.tier.clone();
    }

    let pedidos = tiers_from(&o.tier).unwrap_or_default();
    if semeados.len() == pedidos.len() {
        format!("{}+", o.tier)
    } else {
        semeados.join(", ")
    }
}

fn dataset_de(estado: &Estado, o: &Opcoes) -> Dataset {
    estado.contagens.clone().into_dataset(
        o.regiao.clone(),
        rotulo_de_elo(estado, o),
        agora_iso(),
        o.min_jogos,
    )
}

async fn coletar(riot: Arc<Riot>, o: &Opcoes, estado: &mut Estado) -> Result<()> {
    let filtro = Filter {
        queue: o.fila,
        patch: o.patch.clone(),
    };
    let comeco = Instant::now();
    let mut desde_o_salvamento = 0u64;

    while estado.proximo < estado.jogadores.len() {
        let puuid = estado.jogadores[estado.proximo].clone();
        estado.proximo += 1;

        let ids = match riot.match_ids(&puuid, o.fila, o.por_jogador).await {
            Ok(ids) => ids,
            Err(err) => {
                // Um jogador que falha não pode derrubar o crawl inteiro.
                tracing::warn!(%err, "lista de partidas falhou, seguindo");
                continue;
            }
        };

        let pendentes: Vec<String> = ids
            .into_iter()
            .filter(|id| !estado.vistas.contains(id))
            .collect();

        for lote in pendentes.chunks(o.paralelo) {
            let mut tarefas = JoinSet::new();
            for id in lote {
                let riot = riot.clone();
                let id = id.clone();
                tarefas.spawn(async move {
                    let resultado = riot.match_detail(&id).await;
                    (id, resultado)
                });
            }

            while let Some(terminada) = tarefas.join_next().await {
                let (id, resultado) = terminada.context("tarefa de download")?;
                // Marca como vista mesmo quando falha: insistir numa partida
                // que a Riot não entrega trava o crawl.
                estado.vistas.insert(id);

                match resultado {
                    Ok(Some(partida)) => {
                        let _ = estado.contagens.add(&partida.info, &filtro);
                        desde_o_salvamento += 1;
                    }
                    Ok(None) => {}
                    Err(err) => {
                        if err.to_string().contains("recusou a chave") {
                            return Err(err);
                        }
                        tracing::warn!(%err, "partida falhou, seguindo");
                    }
                }
            }
        }

        if desde_o_salvamento >= SALVAR_A_CADA {
            gravar(&o.estado, estado)?;
            // E a base junto: assim um crawl interrompido deixa sempre um
            // arquivo utilizável no lugar certo, em vez de horas de coleta
            // que só viram base se chegarem ao fim.
            gravar(&o.saida, &dataset_de(estado, o))?;
            desde_o_salvamento = 0;

            let decorrido = comeco.elapsed().as_secs_f64().max(1.0);
            tracing::info!(
                jogadores = format!("{}/{}", estado.proximo, estado.jogadores.len()),
                partidas = estado.contagens.matches,
                vistas = estado.vistas.len(),
                por_minuto = format!("{:.0}", estado.vistas.len() as f64 / decorrido * 60.0),
                "progresso"
            );
        }
    }

    gravar(&o.estado, estado)?;
    Ok(())
}

fn mostrar_descartes(contagens: &Counts) {
    if contagens.skipped.is_empty() {
        return;
    }
    let mut linhas: Vec<(Skip, u64)> = contagens.skipped.iter().map(|(k, v)| (*k, *v)).collect();
    linhas.sort_by_key(|(_, n)| std::cmp::Reverse(*n));

    println!("\nDescartadas");
    for (motivo, quantas) in linhas {
        println!("  {quantas:>8}  {}", motivo.motivo());
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "presage_ingest=info,warn".into()),
        )
        .with_target(false)
        .init();

    let o = ler_opcoes()?;

    if let Some(caminho) = &o.verificar {
        let bruto = std::fs::read(caminho)
            .with_context(|| format!("lendo {}", caminho.display()))?;
        let dataset: Dataset = serde_json::from_slice(&bruto)
            .with_context(|| format!("{} não é um JSON válido", caminho.display()))?;

        // O leitor é tolerante de propósito, para um arquivo truncado ainda
        // carregar. O efeito colateral é que qualquer JSON vira uma base
        // vazia — e aqui isso seria dizer "base pequena" quando o certo é
        // dizer "isto não é uma base".
        if parece_vazio(&dataset) {
            bail!(
                "{} não parece uma base do Presage: não tem patch, nem partidas, \
                 nem nenhuma das três tabelas.",
                caminho.display()
            );
        }

        relatorio(&dataset);
        return Ok(());
    }

    if o.plano {
        return mostrar_plano(&o);
    }

    let regiao = Region::from_platform(&o.regiao).with_context(|| {
        format!(
            "{} não é uma plataforma conhecida. Use uma destas: {}",
            o.regiao,
            Region::platforms().collect::<Vec<_>>().join(", ")
        )
    })?;

    let chave = std::env::var("PRESAGE_RIOT_API_KEY").map_err(|_| {
        anyhow::anyhow!(
            "defina PRESAGE_RIOT_API_KEY antes de rodar.\n\
             No PowerShell: $env:PRESAGE_RIOT_API_KEY = \"RGAPI-...\"\n\
             Rode com --plano para ver o plano sem precisar de chave."
        )
    })?;
    if chave.trim().is_empty() {
        bail!("PRESAGE_RIOT_API_KEY está vazia");
    }

    let riot = Arc::new(Riot::new(
        regiao.platform,
        regiao.routing,
        chave,
        &o.limite,
    )?);

    let mut estado = if o.retomar {
        match std::fs::read(&o.estado).ok().and_then(|bruto| {
            serde_json::from_slice::<Estado>(&bruto).ok()
        }) {
            Some(salvo) if salvo.combina(&o) => {
                tracing::info!(
                    jogadores = format!("{}/{}", salvo.proximo, salvo.jogadores.len()),
                    partidas = salvo.contagens.matches,
                    "retomando"
                );
                salvo
            }
            Some(_) => bail!(
                "o estado em {} é de outro recorte (região, tier ou fila). \
                 Apague-o ou use --estado para um arquivo novo.",
                o.estado.display()
            ),
            None => {
                tracing::info!("sem estado para retomar, começando do zero");
                Estado::default()
            }
        }
    } else {
        Estado::default()
    };

    if estado.jogadores.is_empty() {
        estado.regiao = o.regiao.clone();
        estado.tier = o.tier.clone();
        estado.fila = o.fila;
        tracing::info!("semeando jogadores");
        let (jogadores, tiers) = semear(&riot, &o).await?;
        estado.jogadores = jogadores;
        estado.tiers_semeados = tiers;
        tracing::info!(
            quantos = estado.jogadores.len(),
            elos = estado.tiers_semeados.join(", "),
            "semeadura pronta"
        );
        gravar(&o.estado, &estado)?;
    }

    coletar(riot, &o, &mut estado).await?;

    let dataset = dataset_de(&estado, &o);
    gravar(&o.saida, &dataset)?;

    println!("\nBase gravada em {}", o.saida.display());
    relatorio(&dataset);
    mostrar_descartes(&estado.contagens);
    println!(
        "\n  O Presage lê este arquivo quando abre. Se ele já estiver aberto,\n  \
         feche e abra de novo."
    );

    Ok(())
}

/// ISO 8601 em UTC, sem puxar uma biblioteca de data só para isto.
fn agora_iso() -> String {
    let segundos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let dias = segundos / 86_400;
    let resto = segundos % 86_400;

    // Algoritmo civil-from-days (Howard Hinnant), com a era começando em
    // 0000-03-01 para o ano bissexto cair no fim do ciclo.
    let z = dias as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + i64::from(mes <= 2);

    format!(
        "{ano:04}-{mes:02}-{dia:02}T{:02}:{:02}:{:02}Z",
        resto / 3600,
        (resto % 3600) / 60,
        resto % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_sai_em_iso() {
        let agora = agora_iso();
        assert_eq!(agora.len(), 20, "{agora}");
        assert!(agora.ends_with('Z'));
        assert_eq!(&agora[4..5], "-");
        assert_eq!(&agora[10..11], "T");

        let ano: i64 = agora[..4].parse().unwrap();
        assert!((2024..2100).contains(&ano), "{agora}");
    }

    #[test]
    fn o_estado_so_retoma_o_mesmo_recorte() {
        let o = Opcoes {
            regiao: "br1".into(),
            tier: "EMERALD".into(),
            fila: 420,
            ..Default::default()
        };
        let certo = Estado {
            regiao: "br1".into(),
            tier: "EMERALD".into(),
            fila: 420,
            ..Default::default()
        };
        assert!(certo.combina(&o));

        let outra_regiao = Estado {
            regiao: "euw1".into(),
            ..certo.clone()
        };
        assert!(!outra_regiao.combina(&o));

        let outra_fila = Estado { fila: 440, ..certo };
        assert!(!outra_fila.combina(&o));
    }

    #[test]
    fn a_saida_padrao_e_onde_o_app_procura() {
        let o = Opcoes::default();
        assert_eq!(
            o.saida.file_name().unwrap(),
            "base-de-partidas.json",
            "o nome tem de bater com DATASET_CACHE em src-tauri/src/lib.rs"
        );
        assert!(o.saida.ends_with("Presage/base-de-partidas.json")
            || o.saida.ends_with("Presage\\base-de-partidas.json"));
    }

    #[test]
    fn o_estado_nao_vai_para_o_mesmo_arquivo_da_base() {
        let o = Opcoes::default();
        assert_ne!(o.saida, o.estado);
    }

    #[test]
    fn o_estado_sobrevive_a_ida_e_volta_em_json() {
        let mut estado = Estado {
            regiao: "br1".into(),
            tier: "EMERALD".into(),
            fila: 420,
            jogadores: vec!["puuid-a".into(), "puuid-b".into()],
            proximo: 1,
            ..Default::default()
        };
        estado.vistas.insert("BR1_123".into());
        estado.contagens.matches = 7;
        estado.contagens.skipped.insert(Skip::Remake, 3);

        let texto = serde_json::to_string(&estado).unwrap();
        let voltou: Estado = serde_json::from_str(&texto).unwrap();

        assert_eq!(voltou.proximo, 1);
        assert_eq!(voltou.contagens.matches, 7);
        assert_eq!(voltou.contagens.skipped[&Skip::Remake], 3);
        assert!(voltou.vistas.contains("BR1_123"));
    }

    #[test]
    fn o_rotulo_de_elo_so_diz_mais_quando_a_semeadura_chegou_la() {
        let o = Opcoes {
            tier: "EMERALD".into(),
            ..Default::default()
        };

        // A cota acabou em esmeralda: o cabeçalho não pode prometer o resto.
        let parou_cedo = Estado {
            tiers_semeados: vec!["EMERALD".into()],
            ..Default::default()
        };
        assert_eq!(rotulo_de_elo(&parou_cedo, &o), "EMERALD");

        let parcial = Estado {
            tiers_semeados: vec!["EMERALD".into(), "DIAMOND".into()],
            ..Default::default()
        };
        assert_eq!(rotulo_de_elo(&parcial, &o), "EMERALD, DIAMOND");

        // Chegou até o topo: aí o `+` é verdade.
        let completo = Estado {
            tiers_semeados: tiers_from("EMERALD")
                .unwrap()
                .iter()
                .map(|t| t.to_string())
                .collect(),
            ..Default::default()
        };
        assert_eq!(rotulo_de_elo(&completo, &o), "EMERALD+");
    }

    #[test]
    fn json_qualquer_nao_passa_por_base() {
        // O leitor é tolerante para aguentar arquivo truncado, e o preço é
        // que `{"isto":"nao e uma base"}` desserializa numa base vazia.
        let qualquer: Dataset = serde_json::from_str(r#"{"isto":"nao e uma base"}"#).unwrap();
        assert!(parece_vazio(&qualquer));

        let truncada: Dataset = serde_json::from_str(r#"{"patch":"15.21","matches":0}"#).unwrap();
        assert!(
            !parece_vazio(&truncada),
            "base truncada ainda é uma base, e o relatório deve mostrá-la"
        );
    }

    #[test]
    fn base_de_verdade_nao_e_confundida_com_vazia() {
        let mut dataset = Dataset {
            patch: "15.21".into(),
            matches: 1000,
            ..Default::default()
        };
        assert!(!parece_vazio(&dataset));

        dataset.matches = 0;
        dataset.patch = String::new();
        dataset
            .base
            .insert("103:Mid".into(), presage_core::engine::stats::Record::new(10, 5));
        assert!(!parece_vazio(&dataset), "tem tabela, então é uma base");
    }

    #[test]
    fn o_limite_padrao_e_o_da_chave_de_desenvolvedor() {
        // Começar otimista é o jeito de tomar 429 na primeira rajada.
        let l = Limiter::parse(LIMITE_PADRAO).unwrap();
        assert!(l.sustained_rate() < 1.0);
    }
}
