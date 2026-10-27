export interface DraftSlot {
  cellId: number;
  position: string;
  championId: number;
  intentId: number;
  spell1: number;
  spell2: number;
  isLocal: boolean;
  isActing: boolean;
  locked: boolean;
}

export interface DraftState {
  phase: string;
  timeLeftMs: number;
  totalTimeMs: number;
  localCellId: number;
  localPosition: string;
  myTeam: DraftSlot[];
  theirTeam: DraftSlot[];
  myBans: number[];
  theirBans: number[];
  currentAction: "pick" | "ban" | null;
  isLocalTurn: boolean;
  unavailable: number[];
}

export interface ConnStatus {
  connected: boolean;
  phase: string;
  summoner: string | null;
  detail: string | null;
}

export interface ScoredChampion {
  championId: number;
  score: number;
  delta: number;
  winrate: number;
  confidence: number;
  sample: number;
  inPool: boolean;
  reasons: string[];
}

export interface LiveScores {
  kills: number;
  deaths: number;
  assists: number;
  creepScore: number;
  wardScore: number;
}

export interface LivePlayer {
  championKey: string;
  championId: number;
  displayName: string;
  team: string;
  position: string;
  level: number;
  items: number[];
  goldInvested: number;
  isDead: boolean;
  respawnTimer: number;
  isLocal: boolean;
  scores: LiveScores;
}

export interface LiveGame {
  gameTime: number;
  players: LivePlayer[];
}

export interface Spawn {
  name: string;
  at: number;
  secondsLeft: number;
  /** true quando sai de um abate observado; false quando é a constante do patch. */
  observed: boolean;
}

export interface Sides {
  ally: number;
  enemy: number;
}

export interface Timeline {
  gameTime: number;
  dragons: Sides;
  dragonTypes: string[];
  soul: string | null;
  turrets: Sides;
  kills: Sides;
  upcoming: Spawn[];
}

export interface LanePriority {
  lane: string;
  score: number;
  skirmishOdds: number;
  reasons: string[];
}

export interface PriorityReport {
  lanes: LanePriority[];
  focus: string | null;
}

export interface LiveSnapshot {
  game: LiveGame;
  timeline: Timeline;
  priority: PriorityReport;
}

export interface BuildContext {
  incomingPhysicalShare: number;
  enemyArmorPenPercent: number;
  enemyLethality: number;
  enemyMagicPenPercent: number;
  yourEffectiveHp: number;
}

export interface ItemSuggestion {
  itemId: number;
  name: string;
  cost: number;
  valuePer1000g: number;
  ehpGain: number;
  offenseGainPercent: number;
  reasons: string[];
}

export interface LevelOdds {
  level: number;
  /** "early game", "mid game", "late game". */
  phase: string;
  odds: number;
}

export interface TeamOutlook {
  ours: number[];
  theirs: number[];
  odds: number;
  byLevel: LevelOdds[];
  reasons: string[];
}

export interface LaneOutlook {
  role: Role;
  lane: string;
  ally: number;
  enemy: number;
  allyName: string;
  enemyName: string;
  /** 0 a 1. */
  winProbability: number;
  /** Pontos percentuais acima ou abaixo de uma rota equilibrada. */
  margin: number;
  sample: number;
  /** false quando o número veio só da matemática do duelo. */
  fromData: boolean;
  /** A mesma conta nos níveis 3, 6, 11 e 16, com os dois nivelados. */
  byLevel: LevelOdds[];
  reasons: string[];
}

export interface Skirmish {
  /** Identificador estável do agrupamento: jg_top, jg_mid, bot, dragao, arauto. */
  key: string;
  label: string;
  detail: string;
  size: number;
  ours: number[];
  theirs: number[];
  odds: number;
  byLevel: LevelOdds[];
  reasons: string[];
}

export interface SkirmishFocus {
  skirmish: Skirmish;
  /** Por que esta e não outra: "Dragão em 0:40", "a briga da sua rota". */
  because: string;
}

export interface LaneReport {
  /** Os cinco contra os cinco. null enquanto os lados têm tamanhos diferentes. */
  team: TeamOutlook | null;
  lanes: LaneOutlook[];
  skirmishes: Skirmish[];
  source: "draft" | "partida";
  /** As rotas inimigas foram presumidas pela ordem do draft. */
  rolesAssumed: boolean;
}

export interface GoldPlayer {
  championKey: string;
  championId: number;
  displayName: string;
  team: string;
  position: string;
  isLocal: boolean;
  isAlly: boolean;
  /** Exato: soma dos preços dos itens. */
  invested: number;
  /** Estimado. */
  earned: number;
  unspent: number;
  goldPerMinute: number;
}

export interface GoldReport {
  gameTime: number;
  players: GoldPlayer[];
  oursInvested: number;
  theirsInvested: number;
  investedLead: number;
  oursEarned: number;
  theirsEarned: number;
  earnedLead: number;
  notes: string[];
}

export interface CoachNote {
  verdict: "good" | "bad";
  title: string;
  detail: string;
  weight: number;
}

export interface Point {
  t: number;
  v: number;
}

export interface Series {
  /** Ouro em itens do seu time menos o deles. Exato. */
  goldLead: Point[];
  csDiff: Point[];
  levelDiff: Point[];
  /** Instantes em que você morreu. */
  deaths: number[];
  teamKills: number[];
}

export interface CoachReport {
  champion: string;
  duration: number;
  won: boolean | null;
  good: CoachNote[];
  bad: CoachNote[];
  summary: string;
  series: Series;
}

export type Gap = "magico" | "fisico" | "frontline" | "alcance";

export const GAP_TEXTO: Record<Gap, string> = {
  magico: "sua pool é quase toda de dano físico",
  fisico: "sua pool é quase toda de dano mágico",
  frontline: "nenhum dos seus segura a linha de frente",
  alcance: "todos os seus são corpo a corpo",
};

export interface PoolCandidate {
  championId: number;
  name: string;
  /** Índice de encaixe, de 0 a 1. Não é winrate. */
  score: number;
  fills: Gap[];
  reasons: string[];
}

export interface RuneSuggestion {
  key: string;
  id: number;
  name: string;
  /** Caminho do ícone no CDN do Data Dragon. */
  icon: string;
  treeName: string;
  score: number;
  reasons: string[];
}

export interface RunePlan {
  keystones: RuneSuggestion[];
  secondary: RuneSuggestion | null;
  matchup: string;
  /** Falso enquanto não houver base de partidas por trás. */
  fromData: boolean;
}

export type OverlaySection =
  | "gold"
  | "damage"
  | "objectives"
  | "priority"
  | "skirmish"
  | "nextItem";

/** Guarda o que está **escondido**, não o que está ligado: assim uma seção
 *  nova nasce visível para quem já tem configuração salva. */
export interface OverlayPrefs {
  hidden: OverlaySection[];
}

export const OVERLAY_SECTIONS: {
  id: OverlaySection;
  label: string;
  detail: string;
}[] = [
  {
    id: "gold",
    label: "Ouro em itens",
    detail: "a diferença entre os times, soma exata dos inventários",
  },
  {
    id: "damage",
    label: "Dano que vem contra você",
    detail: "quanto é físico, sua vida efetiva e a penetração deles",
  },
  { id: "objectives", label: "Objetivos", detail: "dragão, barão, arauto e grubs" },
  {
    id: "priority",
    label: "Prioridade de rota",
    detail: "qual rota vale mais agora, com o motivo",
  },
  { id: "skirmish", label: "Escaramuça", detail: "a briga que importa neste minuto" },
  {
    id: "nextItem",
    label: "Próximo item",
    detail: "duas opções, contra o que eles têm agora",
  },
];

export type Role = "Top" | "Jungle" | "Mid" | "Bot" | "Support" | "Unknown";
export type Tier = "core" | "situational" | "extra";

export interface PoolEntry {
  championId: number;
  role: Role | null;
  tier: Tier;
}

export interface Pool {
  entries: PoolEntry[];
}

export const ROLES: { id: Role; label: string }[] = [
  { id: "Top", label: "TOP" },
  { id: "Jungle", label: "JUNGLE" },
  { id: "Mid", label: "MID" },
  { id: "Bot", label: "ADC" },
  { id: "Support", label: "SUPORTE" },
];

/** O Rust lê a rota com `Role::from_lcu`, que espera o vocabulário do cliente
 *  — `middle`, não `mid`. Converter aqui evita que a rota chegue como
 *  desconhecida e o backend devolva a pool errada em silêncio. */
export const ROLE_TO_LCU: Record<Exclude<Role, "Unknown">, string> = {
  Top: "top",
  Jungle: "jungle",
  Mid: "middle",
  Bot: "bottom",
  Support: "utility",
};

export const TIERS: { id: Tier; label: string }[] = [
  { id: "core", label: "Core" },
  { id: "situational", label: "Situacionais" },
  { id: "extra", label: "Extras" },
];

export interface MinimaxPick {
  championId: number;
  greedy: number;
  afterResponse: number;
  /** Quanto o contra-pick tira. Maior = pick mais arriscado. */
  exposure: number;
  bestResponse: number | null;
  inPool: boolean;
}

export interface BanSuggestion {
  championId: number;
  threat: number;
  reason: string;
}

export interface EngineStatus {
  ready: boolean;
  version: string;
  champions: number;
  hasGlobal: boolean;
  hasPersonal: boolean;
  personalGames: number;
  account: string | null;
  globalPatch: string | null;
  globalMatches: number;
}

export const POSITION_LABEL: Record<string, string> = {
  top: "TOP",
  jungle: "JG",
  middle: "MID",
  bottom: "ADC",
  utility: "SUP",
  "": "—",
};
