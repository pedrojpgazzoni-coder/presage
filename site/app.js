// Interatividade da página.
//
// A demonstração da build usa a mesma matemática do motor em Rust: ordem de
// penetração, multiplicador de resistência e vida efetiva contra a mistura de
// dano do time inimigo. É demonstração, não o produto — mas as contas são as
// de verdade, então o comportamento que a pessoa vê é o comportamento real.

(() => {
  "use strict";

  // ---- matemática (espelha core/src/engine/damage.rs) ----

  const multiplicador = (r) => (r >= 0 ? 100 / (100 + r) : 2 - 100 / (100 - r));

  function resistenciaEfetiva(base, pen) {
    let r = base;
    if (r > 0) r *= 1 - (pen.pct ?? 0);
    if (r <= 0) return r;
    return Math.max(0, r - (pen.flat ?? 0));
  }

  // Penetração percentual é multiplicativa: duas fontes de 35% dão 57,75%.
  const juntarPct = (a, b) => 1 - (1 - a) * (1 - b);

  const somar = (itens) =>
    itens.reduce(
      (t, i) => ({
        hp: t.hp + (i.hp ?? 0),
        armor: t.armor + (i.armor ?? 0),
        mr: t.mr + (i.mr ?? 0),
        ad: t.ad + (i.ad ?? 0),
        ap: t.ap + (i.ap ?? 0),
        let: t.let + (i.let ?? 0),
        penArm: juntarPct(t.penArm, i.penArm ?? 0),
        penMag: juntarPct(t.penMag, i.penMag ?? 0),
      }),
      { hp: 0, armor: 0, mr: 0, ad: 0, ap: 0, let: 0, penArm: 0, penMag: 0 }
    );

  // ---- dados ----

  // A partida corre sozinha. Cada entrada é uma compra do time inimigo no
  // minuto em que ela costuma sair — a ordem importa, porque o ranking muda
  // de verdade quando a penetração entra.
  const COMPRAS = [
    { min: "12:40", quem: "Caitlyn", item: 3031, nome: "Lâmina do Infinito", ad: 70 },
    { min: "16:05", quem: "Zed", item: 3142, nome: "Lâmina do Fantasma", ad: 55, let: 18 },
    { min: "21:30", quem: "Caitlyn", item: 3036, nome: "Lord Dominik's", ad: 35, penArm: 0.35 },
    { min: "25:10", quem: "Malphite", item: 3075, nome: "Malha de Espinhos", armor: 70, hp: 350 },
    { min: "29:45", quem: "Viego", item: 3135, nome: "Cajado do Vazio", ap: 95, penMag: 0.4 },
    { min: "33:20", quem: "Thresh", item: 3089, nome: "Chapéu de Rabadon", ap: 130 },
  ];

  const MEUS_CANDIDATOS = [
    { item: 3157, nome: "Ampulheta de Zhonya", custo: 3250, ap: 105, armor: 50 },
    { item: 3116, nome: "Cetro de Cristal de Rylai", custo: 2600, ap: 75, hp: 400 },
    { item: 3135, nome: "Cajado do Vazio", custo: 3000, ap: 95, penMag: 0.4 },
    { item: 3089, nome: "Chapéu Acadêmico de Rabadon", custo: 3600, ap: 130 },
    { item: 3102, nome: "Véu da Banshee", custo: 3000, ap: 100, mr: 50 },
    { item: 4637, nome: "Abraço do Demônio", custo: 3000, ap: 80, hp: 450 },
  ];

  // Mago de nível 13 com dois itens já comprados.
  const EU = { hp: 1980, armor: 74, mr: 52, ad: 92, ap: 180 };
  // Resistências médias do time inimigo naquele ponto da partida.
  const BASE_INIMIGA = { armor: 78, mr: 54 };

  // Quantas compras já aconteceram. Começa com a primeira na mesa.
  let compradas = 1;

  // ---- cálculo ----

  function calcular() {
    const escolhidos = COMPRAS.slice(0, compradas);
    const deles = somar(escolhidos);

    const danoDeles = deles.ad + deles.ap;
    const parteFisica = danoDeles > 0 ? deles.ad / danoDeles : 0.6;

    const penDelesFisica = { pct: deles.penArm, flat: deles.let };
    const penDelesMagica = { pct: deles.penMag, flat: 0 };

    const alvo = {
      armor: BASE_INIMIGA.armor + deles.armor,
      mr: BASE_INIMIGA.mr + deles.mr,
    };

    const poder = (meu) => {
      const tomado =
        parteFisica * multiplicador(resistenciaEfetiva(meu.armor, penDelesFisica)) +
        (1 - parteFisica) * multiplicador(resistenciaEfetiva(meu.mr, penDelesMagica));
      const vidaEfetiva = meu.hp / tomado;

      const minhaPenMag = { pct: meu.penMag ?? 0, flat: 0 };
      const ofensiva =
        meu.ad * multiplicador(alvo.armor) + meu.ap * multiplicador(resistenciaEfetiva(alvo.mr, minhaPenMag));

      return { poder: vidaEfetiva * ofensiva, vidaEfetiva };
    };

    const base = poder({ ...EU, penMag: 0 });

    const ranking = MEUS_CANDIDATOS.map((item) => {
      const com = poder({
        hp: EU.hp + (item.hp ?? 0),
        armor: EU.armor + (item.armor ?? 0),
        mr: EU.mr + (item.mr ?? 0),
        ad: EU.ad,
        ap: EU.ap + (item.ap ?? 0),
        penMag: item.penMag ?? 0,
      });
      const ganho = (com.poder / base.poder - 1) * 100;
      return { ...item, valor: (ganho * 1000) / item.custo };
    }).sort((a, b) => b.valor - a.valor);

    return { parteFisica, deles, ranking, vidaEfetiva: base.vidaEfetiva };
  }

  // ---- artes do Data Dragon ----
  //
  // A versão vem do próprio CDN, então a página nunca aponta para um patch
  // antigo. Sem rede, os retratos ficam com o fundo sólido e nada quebra.

  // Só a consulta da versão passa por CORS; as imagens, não. Abrindo a página
  // direto do disco a consulta pode ser recusada, então cai num patch fixo em
  // vez de ficar sem retrato nenhum.
  const PATCH_RESERVA = "16.21.1";
  let baseArte = null;

  const cdn = (versao) => `https://ddragon.leagueoflegends.com/cdn/${versao}/img`;

  async function carregarArtes() {
    baseArte = cdn(PATCH_RESERVA);
    aplicarArtes(document);

    try {
      const versoes = await fetch("https://ddragon.leagueoflegends.com/api/versions.json").then((r) =>
        r.json()
      );
      if (versoes?.[0] && versoes[0] !== PATCH_RESERVA) {
        baseArte = cdn(versoes[0]);
        aplicarArtes(document);
      }
    } catch {
      /* fica no patch de reserva */
    }
  }

  function aplicarArtes(raiz) {
    if (!baseArte) return;
    for (const el of raiz.querySelectorAll("[data-champ]")) {
      el.style.backgroundImage = `url(${baseArte}/champion/${el.dataset.champ}.png)`;
      el.classList.add("com-arte");
    }
    for (const el of raiz.querySelectorAll("[data-item]")) {
      el.style.backgroundImage = `url(${baseArte}/item/${el.dataset.item}.png)`;
      el.classList.add("com-arte");
    }
  }

  // ---- interface ----

  const $ = (id) => document.getElementById(id);

  function montarChips() {
    const alvo = $("demo-inimigo");
    if (!alvo) return;

    // Só acrescenta o que falta. Redesenhar a lista inteira reiniciaria a
    // animação de todas as compras a cada tique.
    const jaNaTela = alvo.children.length;
    if (compradas < jaNaTela) alvo.innerHTML = "";

    for (const el of alvo.children) el.classList.remove("novo");

    for (let i = alvo.children.length; i < compradas; i++) {
      const c = COMPRAS[i];
      const span = document.createElement("span");
      span.className = "chip chip-live" + (i === compradas - 1 ? " novo" : "");
      span.innerHTML = `<i class="face xs square" data-item="${c.item}"></i>${c.nome}`;
      span.title = `${c.quem} comprou aos ${c.min}`;
      alvo.appendChild(span);
    }

    const relogio = $("demo-relogio");
    if (relogio) relogio.textContent = COMPRAS[compradas - 1].min;

    aplicarArtes(alvo);
  }

  // A partida anda sozinha e recomeça no fim. Fica parada enquanto a seção
  // está fora da tela: ninguém precisa ver a animação pelas costas.
  function rodarPartida() {
    const painel = $("demo-inimigo")?.closest(".panel-demo");
    if (!painel) return;

    let timer = null;

    const passo = () => {
      compradas = compradas >= COMPRAS.length ? 1 : compradas + 1;
      if (compradas === 1) {
        const alvo = $("demo-inimigo");
        if (alvo) alvo.innerHTML = "";
      }
      montarChips();
      desenhar();
      timer = setTimeout(passo, compradas === COMPRAS.length ? 4200 : 2600);
    };

    const ligar = () => {
      if (!timer) timer = setTimeout(passo, 2600);
    };
    const desligar = () => {
      clearTimeout(timer);
      timer = null;
    };

    if (!("IntersectionObserver" in window)) return ligar();

    new IntersectionObserver((e) => (e[0].isIntersecting ? ligar() : desligar()), {
      threshold: 0.25,
    }).observe(painel);
  }

  function desenhar() {
    const r = calcular();
    const fis = Math.round(r.parteFisica * 100);

    const barra = $("demo-split");
    if (barra) barra.style.width = `${fis}%`;
    if ($("demo-fis")) $("demo-fis").textContent = `${fis}% físico`;
    if ($("demo-mag")) $("demo-mag").textContent = `${100 - fis}% mágico`;

    const fatos = $("demo-fatos");
    if (fatos) {
      const linhas = [
        ["Sua vida efetiva", Math.round(r.vidaEfetiva).toLocaleString("pt-BR"), ""],
      ];
      if (r.deles.penArm > 0)
        linhas.push(["Pen. de armadura deles", `${Math.round(r.deles.penArm * 100)}%`, "bad"]);
      if (r.deles.let > 0) linhas.push(["Letalidade deles", String(r.deles.let), "bad"]);
      if (r.deles.penMag > 0)
        linhas.push(["Pen. mágica deles", `${Math.round(r.deles.penMag * 100)}%`, "bad"]);

      fatos.innerHTML = linhas
        .map(([k, v, c]) => `<div class="fact"><small>${k}</small><b class="${c}">${v}</b></div>`)
        .join("");
    }

    const lista = $("demo-itens");
    if (!lista) return;
    lista.innerHTML = r.ranking
      .slice(0, 4)
      .map(
        (item, i) => `
        <div class="row demo-row" style="--i:${i}">
          <span class="face sm square" data-item="${item.item}"></span>
          <span class="row-name">${item.nome}<small>${item.custo} ouro</small></span>
          <span class="row-val good">${item.valor.toFixed(1)}</span>
        </div>`
      )
      .join("");
    aplicarArtes(lista);
  }

  // ---- chance por rota e escaramuça ----

  // `niveis` são as chances nos níveis 3, 6, 11 e 16, com os dois nivelados.
  const ROTAS = [
    { rota: "TOP", eu: "Garen", ele: "Malphite", p: 0.43, niveis: [0.49, 0.47, 0.42, 0.4], por: "Perde para Malphite na rota (−3.1pp, n=2410)" },
    { rota: "JG", eu: "LeeSin", ele: "Viego", p: 0.56, niveis: [0.61, 0.58, 0.54, 0.5], por: "Ganha o 1v1 com os itens de agora (58%)" },
    { rota: "MID", eu: "Ahri", ele: "Zed", p: 0.47, niveis: [0.52, 0.44, 0.48, 0.55], por: "Ahri tem alcance; Zed precisa chegar perto" },
    { rota: "ADC", eu: "Ashe", ele: "Caitlyn", p: 0.41, niveis: [0.38, 0.4, 0.43, 0.47], por: "Perde para Caitlyn na rota (−2.2pp, n=3100)" },
    { rota: "SUP", eu: "Thresh", ele: "Lulu", p: 0.62, niveis: [0.58, 0.63, 0.64, 0.6], por: "Vence Lulu na rota (+4.0pp, n=1450)" },
  ];

  // As três fases, com o nível que representa cada uma. A última leva "+":
  // o late game não acaba no 11, começa nele.
  const FASES = [
    { nome: "early game", nivel: "nv 3" },
    { nome: "mid game", nivel: "nv 6" },
    { nome: "late game", nivel: "nv 11+" },
  ];

  const BRIGAS = [
    {
      rotulo: "JG+TOP x JG+TOP",
      quando: "gank ou invade no topo",
      nossos: ["LeeSin", "Garen"],
      deles: ["Viego", "Malphite"],
      p: 0.48,
      por: "Quem mais pesa do seu lado: Garen",
    },
    {
      rotulo: "JG+MID x JG+MID",
      quando: "gank ou briga no meio",
      nossos: ["LeeSin", "Ahri"],
      deles: ["Viego", "Zed"],
      p: 0.46,
      por: "Eles têm frontline no grupo e vocês não",
    },
    {
      rotulo: "ADC+SUP x ADC+SUP",
      quando: "rota inferior, sem jungle",
      nossos: ["Ashe", "Thresh"],
      deles: ["Caitlyn", "Lulu"],
      p: 0.53,
      por: "Do lado deles: Caitlyn",
    },
    {
      rotulo: "JG+ADC+SUP x JG+ADC+SUP",
      quando: "briga no dragão",
      nossos: ["LeeSin", "Ashe", "Thresh"],
      deles: ["Viego", "Caitlyn", "Lulu"],
      p: 0.61,
      por: "Vocês têm frontline e eles não",
    },
    {
      rotulo: "JG+TOP+MID x JG+TOP+MID",
      quando: "briga no arauto",
      nossos: ["LeeSin", "Garen", "Ahri"],
      deles: ["Viego", "Malphite", "Zed"],
      p: 0.45,
      por: "Do lado deles: Malphite",
    },
  ];

  // Verde acima de 55%, coral abaixo de 45%, neutro no meio. A cor da marca
  // nunca entra aqui: violeta é identidade, não resultado.
  const cor = (p) => (p >= 0.55 ? "good" : p <= 0.45 ? "bad" : "even");
  const retrato = (nome) => `<i class="face xs" data-champ="${nome}"></i>`;

  function montarRotas() {
    const alvo = $("demo-rotas");
    if (!alvo) return;

    alvo.innerHTML = ROTAS.map(
      (r) => `
      <div class="rota">
        <div class="rota-head">
          <span class="rota-tag">${r.rota}</span>
          ${retrato(r.eu)}<em>x</em>${retrato(r.ele)}
          <b class="${cor(r.p)}">${Math.round(r.p * 100)}%</b>
        </div>
        <div class="bar"><span class="${cor(r.p)}" style="width:${Math.round(r.p * 100)}%"></span></div>
        <div class="niveis">
          ${r.niveis
            .slice(0, FASES.length)
            .map(
              (p, i) =>
                `<span class="${cor(p)}"><small>${FASES[i].nome} <i>${FASES[i].nivel}</i></small>${Math.round(p * 100)}%</span>`
            )
            .join("")}
        </div>
        <small>${r.por}</small>
      </div>`
    ).join("");

    aplicarArtes(alvo);
  }

  // A caixa de escaramuça troca sozinha de agrupamento. É o comportamento do
  // app: ninguém escolhe qual briga olhar, elas aparecem.
  function rodarBrigas() {
    const alvo = $("demo-briga");
    if (!alvo) return;

    let i = 0;
    let timer = null;

    const desenhar = () => {
      const b = BRIGAS[i];
      const meu = Math.round(b.p * 100);
      alvo.innerHTML = `
        <div class="briga-head">
          <span class="briga-rotulo">${b.rotulo}</span>
        </div>
        <div class="briga-faces">
          ${b.nossos.map(retrato).join("")}<em>x</em>${b.deles.map(retrato).join("")}
        </div>
        <div class="bar"><span class="${cor(b.p)}" style="width:${meu}%"></span></div>
        <div class="briga-lados">
          <b class="${cor(b.p)}">Vocês ${meu}%</b>
          <span>Eles ${100 - meu}%</span>
        </div>
        <small>${b.quando} · ${b.por}</small>
        <div class="briga-pontos">
          ${BRIGAS.map((_, n) => `<i class="${n === i ? "on" : ""}"></i>`).join("")}
        </div>`;
      aplicarArtes(alvo);
    };

    const passo = () => {
      i = (i + 1) % BRIGAS.length;
      desenhar();
    };

    desenhar();

    const ligar = () => {
      if (!timer) timer = setInterval(passo, 3200);
    };
    const desligar = () => {
      clearInterval(timer);
      timer = null;
    };

    if (!("IntersectionObserver" in window)) return ligar();
    new IntersectionObserver((e) => (e[0].isIntersecting ? ligar() : desligar()), {
      threshold: 0.3,
    }).observe(alvo);
  }

  // ---- relógio dos objetivos ----

  function relogio() {
    const alvos = document.querySelectorAll("[data-contagem]");
    if (!alvos.length) return;

    setInterval(() => {
      for (const el of alvos) {
        let s = Number(el.dataset.contagem) - 1;
        if (s < 0) s = Number(el.dataset.reinicia || 300);
        el.dataset.contagem = String(s);
        el.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
        el.classList.toggle("warn", s <= 60);
      }
    }, 1000);
  }

  // ---- revelação no scroll ----

  function revelar() {
    const alvos = document.querySelectorAll(".feature, .card, .step, .overlay-sec, .download");
    if (!("IntersectionObserver" in window) || window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      alvos.forEach((e) => e.classList.add("visivel"));
      return;
    }

    const obs = new IntersectionObserver(
      (entradas) => {
        for (const e of entradas) {
          if (e.isIntersecting) {
            e.target.classList.add("visivel");
            obs.unobserve(e.target);
          }
        }
      },
      { rootMargin: "0px 0px -12% 0px" }
    );

    alvos.forEach((e) => {
      e.classList.add("revelar");
      obs.observe(e);
    });
  }

  // Marca que o JS está vivo. O CSS só esconde para revelar depois sob esta
  // classe — sem script, ou se algo aqui quebrar, a página aparece inteira em
  // vez de ficar em branco.
  document.documentElement.classList.add("js");

  function iniciar() {
    montarChips();
    desenhar();
    rodarPartida();
    montarRotas();
    rodarBrigas();
    relogio();
    revelar();
    carregarArtes();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", iniciar);
  } else {
    iniciar();
  }
})();
