import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { loadStaticData, type StaticData } from "./lib/ddragon";
import type {
  BanSuggestion,
  BuildContext,
  CoachReport,
  ConnStatus,
  DraftState,
  EngineStatus,
  Gap,
  GoldReport,
  ItemSuggestion,
  LaneReport,
  LiveSnapshot,
  MinimaxPick,
  OverlayPrefs,
  Pool,
  PoolCandidate,
  Profile,
  Role,
  RunePlan,
  ScoredChampion,
} from "./lib/types";
import { MAPA_PADRAO, ROLE_TO_LCU } from "./lib/types";
import Atualizacao from "./components/Atualizacao";
import ConnectionStatus from "./components/ConnectionStatus";
import DraftBoard from "./components/DraftBoard";
import GoldTracker from "./components/GoldTracker";
import Home from "./components/Home";
import LaneAnalysis from "./components/LaneAnalysis";
import Onboarding from "./components/Onboarding";
import LiveBuild from "./components/LiveBuild";
import Logo from "./components/Logo";
import LiveOverview from "./components/LiveOverview";
import OverlayPrefsPanel from "./components/OverlayPrefsPanel";
import PoolBuilder from "./components/PoolBuilder";
import PostGame from "./components/PostGame";
import RecommendationPanel from "./components/RecommendationPanel";
import Runas from "./components/Runas";

const EVENT_STATUS = "presage://status";
const EVENT_DRAFT = "presage://draft";
const EVENT_ENGINE = "presage://engine";
const EVENT_LIVE = "presage://live";
const EVENT_REVIEW = "presage://review";
const EVENT_PROFILE = "presage://profile";

type View = "home" | "draft" | "lanes" | "live" | "coach" | "pool" | "overlay";

const ABAS: { id: View; label: string }[] = [
  { id: "home", label: "Início" },
  { id: "draft", label: "Draft" },
  { id: "lanes", label: "Rotas" },
  { id: "live", label: "Partida" },
  { id: "coach", label: "Pós-jogo" },
  { id: "pool", label: "Pool" },
  { id: "overlay", label: "Overlay" },
];

export default function App() {
  const [view, setView] = useState<View>("home");
  const [profile, setProfile] = useState<Profile | null>(null);
  // `null` enquanto não sabemos: mostrar a tela de boas-vindas por um instante
  // para quem já a respondeu seria pior que esperar a resposta.
  const [primeiraVez, setPrimeiraVez] = useState<boolean | null>(null);
  const [status, setStatus] = useState<ConnStatus | null>(null);
  const [draft, setDraft] = useState<DraftState | null>(null);
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [pool, setPool] = useState<Pool>({ entries: [] });
  const [recommendations, setRecommendations] = useState<ScoredChampion[]>([]);
  const [minimax, setMinimax] = useState<MinimaxPick[]>([]);
  const [bans, setBans] = useState<BanSuggestion[]>([]);
  const [live, setLive] = useState<LiveSnapshot | null>(null);
  const [buildContext, setBuildContext] = useState<BuildContext | null>(null);
  const [suggestions, setSuggestions] = useState<ItemSuggestion[]>([]);
  const [lanes, setLanes] = useState<LaneReport | null>(null);
  const [goldReport, setGoldReport] = useState<GoldReport | null>(null);
  const [review, setReview] = useState<CoachReport | null>(null);
  const [runas, setRunas] = useState<RunePlan | null>(null);
  const [overlayPrefs, setOverlayPrefs] = useState<OverlayPrefs>({
    hidden: [],
    map: MAPA_PADRAO,
  });
  const [staticData, setStaticData] = useState<StaticData | null>(null);
  const [staticError, setStaticError] = useState<string | null>(null);

  const clockAnchor = useRef<{ at: number; leftMs: number } | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    loadStaticData()
      .then(setStaticData)
      .catch((err: unknown) => setStaticError(String(err)));
  }, []);

  useEffect(() => {
    let disposed = false;

    invoke<[ConnStatus | null, DraftState | null]>("get_snapshot")
      .then(([snapStatus, snapDraft]) => {
        if (disposed) return;
        if (snapStatus) setStatus(snapStatus);
        if (snapDraft) {
          setDraft(snapDraft);
          clockAnchor.current = { at: Date.now(), leftMs: snapDraft.timeLeftMs };
        }
      })
      .catch(() => void 0);

    invoke<EngineStatus>("engine_status").then(setEngine).catch(() => void 0);
    invoke<Pool>("get_pool").then(setPool).catch(() => void 0);
    invoke<LiveSnapshot | null>("get_live").then(setLive).catch(() => void 0);
    invoke<CoachReport | null>("last_review").then(setReview).catch(() => void 0);
    invoke<OverlayPrefs>("get_overlay_prefs").then(setOverlayPrefs).catch(() => void 0);
    invoke<Profile | null>("lcu_profile").then(setProfile).catch(() => void 0);
    invoke<boolean>("onboarding_done")
      .then((feito) => setPrimeiraVez(!feito))
      // Sem backend (a demonstração roda sem Tauri), não há primeira vez.
      .catch(() => setPrimeiraVez(false));

    const unlisteners = [
      listen<ConnStatus>(EVENT_STATUS, (event) => setStatus(event.payload)),
      listen<EngineStatus>(EVENT_ENGINE, (event) => setEngine(event.payload)),
      listen<LiveSnapshot | null>(EVENT_LIVE, (event) => setLive(event.payload)),
      // A partida acabou e a leitura ficou pronta: traz a aba para a frente,
      // porque é exatamente a hora em que ela interessa.
      listen<CoachReport>(EVENT_REVIEW, (event) => {
        setReview(event.payload);
        setView("coach");
      }),
      listen<Profile>(EVENT_PROFILE, (event) => setProfile(event.payload)),
      listen<DraftState | null>(EVENT_DRAFT, (event) => {
        setDraft(event.payload);
        clockAnchor.current = event.payload
          ? { at: Date.now(), leftMs: event.payload.timeLeftMs }
          : null;
      }),
    ];

    const ticker = window.setInterval(() => setTick((n) => n + 1), 100);

    return () => {
      disposed = true;
      window.clearInterval(ticker);
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => void 0));
    };
  }, []);

  useEffect(() => {
    if (!draft || !engine?.ready) {
      setRecommendations([]);
      setMinimax([]);
      setBans([]);
      return;
    }
    let cancelled = false;

    Promise.all([
      invoke<ScoredChampion[]>("rank_draft"),
      invoke<MinimaxPick[]>("minimax_draft"),
      invoke<BanSuggestion[]>("ban_suggestions"),
    ])
      .then(([ranked, tree, banList]) => {
        if (cancelled) return;
        setRecommendations(ranked);
        setMinimax(tree);
        setBans(banList);
      })
      .catch(() => void 0);

    return () => {
      cancelled = true;
    };
  }, [draft, engine?.ready, pool]);

  useEffect(() => {
    if (!live) {
      setBuildContext(null);
      setSuggestions([]);
      setGoldReport(null);
      return;
    }
    let cancelled = false;

    Promise.all([
      invoke<[BuildContext, ItemSuggestion[]] | null>("build_suggestions", { limit: 8 }),
      invoke<GoldReport | null>("gold_report"),
    ])
      .then(([build, gold]) => {
        if (cancelled) return;
        if (build) {
          setBuildContext(build[0]);
          setSuggestions(build[1]);
        }
        setGoldReport(gold);
      })
      .catch(() => void 0);

    return () => {
      cancelled = true;
    };
  }, [live]);

  // A análise de rota serve os dois momentos: no draft é projeção, na partida
  // sai dos níveis e itens reais. O comando decide sozinho qual fonte usar.
  useEffect(() => {
    if (!engine?.ready || (!draft && !live)) {
      setLanes(null);
      return;
    }
    let cancelled = false;
    Promise.all([
      invoke<LaneReport | null>("lane_analysis"),
      invoke<RunePlan | null>("rune_plan"),
    ])
      .then(([report, plano]) => {
        if (cancelled) return;
        setLanes(report);
        setRunas(plano);
      })
      .catch(() => void 0);
    return () => {
      cancelled = true;
    };
  }, [draft, live, engine?.ready]);

  // O construtor pergunta o que falta só quando chega no último passo.
  const pedirSugestoes = useCallback(
    (role: Role | null) =>
      invoke<[Gap[], PoolCandidate[]]>("pool_suggestions", {
        role: role && role !== "Unknown" ? ROLE_TO_LCU[role] : null,
        limit: 8,
      }),
    []
  );

  // O painel acompanha a partida: o draft começa, a tela vira draft; o jogo
  // começa, vira partida; acabou, volta para o início. Quem está jogando não
  // deveria precisar clicar numa aba para ver o que está acontecendo agora.
  //
  // Clicar numa aba continua valendo — até a fase mudar de novo.
  useEffect(() => {
    if (draft) setView("draft");
  }, [draft !== null]);

  useEffect(() => {
    if (live) setView("live");
  }, [live !== null]);

  useEffect(() => {
    if (!draft && !live) setView((atual) => (atual === "draft" || atual === "live" ? "home" : atual));
  }, [draft, live]);

  const secondsLeft = useMemo(() => {
    const anchor = clockAnchor.current;
    if (!anchor) return null;
    return Math.max(0, (anchor.leftMs - (Date.now() - anchor.at)) / 1000);
  }, [draft, tick]);

  function savePool(next: Pool) {
    setPool(next);
    invoke("set_pool", { pool: next }).catch(() => void 0);
  }

  function terminarPrimeiraVez() {
    setPrimeiraVez(false);
    setView("home");
    invoke("finish_onboarding").catch(() => void 0);
  }

  function saveOverlayPrefs(next: OverlayPrefs) {
    // Otimista: o clique responde na hora e o backend confirma depois. Travar
    // um checkbox esperando ida e volta ao disco seria lento à toa.
    setOverlayPrefs(next);
    invoke("set_overlay_prefs", { prefs: next }).catch(() => void 0);
  }

  // Antes de qualquer outra coisa: a primeira execução toma a tela inteira,
  // sem abas. Quem acabou de instalar não tem o que navegar ainda.
  if (primeiraVez === true) {
    return (
      <div className="app">
        <main className="layout layout-single">
          <section className="panel">
            <Onboarding
              pool={pool}
              staticData={staticData}
              onChange={savePool}
              onSuggest={pedirSugestoes}
              onPronto={terminarPrimeiraVez}
            />
          </section>
        </main>
      </div>
    );
  }

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <Logo size={22} />
          <span className="brand-mark">PRESAGE</span>
          <span className="brand-tag">
            {engine?.ready ? `patch ${engine.version}` : "draft engine"} · v
            {__APP_VERSION__}
          </span>
        </div>
        <nav className="nav">
          {ABAS.map((aba) => (
            <button
              type="button"
              key={aba.id}
              className={view === aba.id ? "on" : ""}
              onClick={() => setView(aba.id)}
            >
              {aba.label}
              {/* O ponto marca onde algo está acontecendo agora. */}
              {aba.id === "live" && live && <span className="nav-dot" />}
              {aba.id === "draft" && draft && <span className="nav-dot" />}
            </button>
          ))}
        </nav>
        <ConnectionStatus status={status} />
      </header>

      <Atualizacao />

      {staticError && (
        <div className="banner banner-warn">
          Não consegui carregar os dados do Data Dragon ({staticError}). Nomes e
          ícones ficam indisponíveis.
        </div>
      )}

      {view === "home" ? (
        <main className="layout layout-single">
          <section className="panel">
            <Home
              profile={profile}
              status={status}
              staticData={staticData}
              onAbrirPool={() => setView("pool")}
            />
          </section>
        </main>
      ) : view === "pool" ? (
        <main className="layout layout-single">
          <section className="panel">
            <PoolBuilder
              pool={pool}
              staticData={staticData}
              onChange={savePool}
              onSuggest={pedirSugestoes}
            />
          </section>
        </main>
      ) : view === "overlay" ? (
        <main className="layout layout-single">
          <section className="panel">
            <OverlayPrefsPanel prefs={overlayPrefs} onChange={saveOverlayPrefs} />
          </section>
        </main>
      ) : view === "lanes" ? (
        <main className="layout layout-single">
          <section className="panel">
            <LaneAnalysis report={lanes} staticData={staticData} />
            <Runas plan={runas} patch={engine?.version ?? null} />
          </section>
        </main>
      ) : view === "coach" ? (
        <main className="layout layout-single">
          <section className="panel">
            <PostGame report={review} />
          </section>
        </main>
      ) : view === "live" ? (
        live ? (
          <main className="layout">
            <section className="panel">
              <LiveOverview snapshot={live} />
              <GoldTracker report={goldReport} staticData={staticData} />
            </section>
            <section className="panel">
              <LiveBuild
                game={live.game}
                context={buildContext}
                suggestions={suggestions}
                staticData={staticData}
              />
            </section>
          </main>
        ) : (
          <main className="layout layout-single">
            <section className="panel">
              <div className="empty">
                <h2>Sem partida em andamento</h2>
                <p>
                  {status?.connected
                    ? "Entre numa partida e os timers e a build adaptativa ligam sozinhos."
                    : "Abra o cliente do League of Legends."}
                </p>
              </div>
            </section>
          </main>
        )
      ) : (
        <main className="layout">
          <section className="panel panel-draft">
            <DraftBoard
              draft={draft}
              staticData={staticData}
              secondsLeft={secondsLeft}
              connected={status?.connected ?? false}
              phase={status?.phase ?? "Disconnected"}
            />
          </section>

          <section className="panel panel-reco">
            <RecommendationPanel
              recommendations={recommendations}
              minimax={minimax}
              bans={bans}
              staticData={staticData}
              draft={draft}
              engine={engine}
            />
          </section>
        </main>
      )}
    </div>
  );
}
