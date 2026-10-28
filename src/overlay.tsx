import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type {
  BuildContext,
  GoldReport,
  ItemSuggestion,
  LiveSnapshot,
  OverlayPrefs,
  OverlaySection,
  SkirmishFocus,
} from "./lib/types";
import { MAPA_PADRAO } from "./lib/types";
import MapaTimers from "./components/MapaTimers";
import "./overlay.css";

const EVENT_LIVE = "presage://live";
const EVENT_PREFS = "presage://prefs";

const BR = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });

function clock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

function Overlay() {
  const [snapshot, setSnapshot] = useState<LiveSnapshot | null>(null);
  // Duas opções de item, não uma. Além de ser mais útil quando a primeira
  // está fora do seu alcance de ouro, a política da Riot pede ferramentas que
  // ofereçam escolhas em vez de ditar a jogada.
  const [itens, setItens] = useState<ItemSuggestion[]>([]);
  const [context, setContext] = useState<BuildContext | null>(null);
  const [gold, setGold] = useState<GoldReport | null>(null);
  const [briga, setBriga] = useState<SkirmishFocus | null>(null);
  // Nada escondido até o backend responder: o atraso de uma consulta não pode
  // piscar um overlay vazio na frente de quem está jogando.
  const [prefs, setPrefs] = useState<OverlayPrefs>({
    hidden: [],
    map: MAPA_PADRAO,
  });

  const mostra = (secao: OverlaySection) => !prefs.hidden.includes(secao);

  useEffect(() => {
    invoke<OverlayPrefs>("get_overlay_prefs").then(setPrefs).catch(() => void 0);
    // Mexer no painel tem efeito imediato, inclusive no meio de uma partida.
    const unlisten = listen<OverlayPrefs>(EVENT_PREFS, (event) => setPrefs(event.payload));
    return () => {
      unlisten.then((un) => un()).catch(() => void 0);
    };
  }, []);

  useEffect(() => {
    invoke<LiveSnapshot | null>("get_live").then(setSnapshot).catch(() => void 0);
    const unlisten = listen<LiveSnapshot | null>(EVENT_LIVE, (event) =>
      setSnapshot(event.payload)
    );
    return () => {
      unlisten.then((un) => un()).catch(() => void 0);
    };
  }, []);

  useEffect(() => {
    if (!snapshot) {
      setItens([]);
      setContext(null);
      setGold(null);
      setBriga(null);
      return;
    }
    let cancelled = false;

    // Tudo se refaz a cada tique da partida: o que eles compraram muda a
    // ordem dos itens, e quem morreu muda a chance da briga.
    Promise.all([
      invoke<[BuildContext, ItemSuggestion[]] | null>("build_suggestions", { limit: 2 }),
      invoke<GoldReport | null>("gold_report"),
      invoke<SkirmishFocus | null>("skirmish_focus"),
    ])
      .then(([build, goldReport, foco]) => {
        if (cancelled) return;
        setContext(build?.[0] ?? null);
        setItens(build?.[1] ?? []);
        setGold(goldReport);
        setBriga(foco);
      })
      .catch(() => void 0);

    return () => {
      cancelled = true;
    };
  }, [snapshot]);

  // Sem partida, ou com tudo desligado, a janela some. Overlay vazio é
  // retângulo escuro por cima do jogo sem dizer nada.
  if (!snapshot || prefs.hidden.length >= 6) {
    return null;
  }

  const top = snapshot.priority.lanes[0];
  const fisico = context ? Math.round(context.incomingPhysicalShare * 100) : null;

  // A janela cobre a tela inteira para poder ancorar em dois cantos: o cartão
  // em cima à esquerda, os relógios em cima do minimapa. Nada aqui pega
  // clique — a janela inteira é atravessável.
  return (
    <div className="hud-tela">
      {mostra("objectives") && (
        <MapaTimers upcoming={snapshot.timeline.upcoming} map={prefs.map} />
      )}

      <div className="hud">
      <div className="hud-head">
        <span className="hud-brand">PRESAGE</span>
        <span className="hud-clock">{clock(snapshot.timeline.gameTime)}</span>
      </div>

      {gold && mostra("gold") && (
        <div className="hud-row">
          <span className="hud-label">Ouro em itens</span>
          <span className={`hud-value ${gold.investedLead >= 0 ? "up" : "down"}`}>
            {gold.investedLead >= 0 ? "+" : "−"}
            {BR.format(Math.abs(gold.investedLead))}
          </span>
        </div>
      )}

      {context && fisico !== null && mostra("damage") && (
        <div className="hud-block">
          <div className="hud-row">
            <span className="hud-label">Dano contra você</span>
            <span className="hud-value">{fisico}% físico</span>
          </div>
          <div className="hud-split">
            <span style={{ width: `${fisico}%` }} />
          </div>
          <div className="hud-row">
            <span className="hud-label">Sua vida efetiva</span>
            <span className="hud-value">{BR.format(context.yourEffectiveHp)}</span>
          </div>
          {context.enemyArmorPenPercent > 0 && (
            <div className="hud-row">
              <span className="hud-label">Pen. de armadura deles</span>
              <span className="hud-value down">
                {Math.round(context.enemyArmorPenPercent * 100)}%
              </span>
            </div>
          )}
        </div>
      )}

      {/* Os objetivos não ficam aqui: eles ficam em cima do minimapa, que é
          para onde os olhos já vão. Ver `MapaTimers`. */}

      {top && mostra("priority") && (
        <div className="hud-block">
          <div className="hud-row">
            <span className="hud-label">Prioridade</span>
            <span className="hud-value accent">{top.lane}</span>
          </div>
          {top.reasons[0] && <div className="hud-note">{top.reasons[0]}</div>}
        </div>
      )}

      {briga && mostra("skirmish") && (
        <div className="hud-block">
          <div className="hud-row">
            <span className="hud-label">{briga.skirmish.label}</span>
            <span
              className={`hud-value ${
                briga.skirmish.odds >= 0.55
                  ? "up"
                  : briga.skirmish.odds <= 0.45
                    ? "down"
                    : ""
              }`}
            >
              {Math.round(briga.skirmish.odds * 100)}%
            </span>
          </div>
          {/* A porcentagem é sempre do seu lado, e o texto diz isso — "45%"
              sozinho não informa de quem é. */}
          <div className="hud-note">
            vocês · {briga.because}
          </div>
        </div>
      )}

      {itens.length > 0 && mostra("nextItem") && (
        <div className="hud-block">
          <div className="hud-row">
            <span className="hud-label">Próximo item</span>
          </div>
          {itens.map((item, i) => (
            <div key={item.itemId}>
              <div className={`hud-note ${i === 0 ? "strong" : ""}`}>
                {item.name}
                <span className="hud-ouro">{BR.format(item.cost)}</span>
              </div>
              {i === 0 && item.reasons[0] && (
                <div className="hud-note">{item.reasons[0]}</div>
              )}
            </div>
          ))}
        </div>
      )}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Overlay />
  </React.StrictMode>
);
