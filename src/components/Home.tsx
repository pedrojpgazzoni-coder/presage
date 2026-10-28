import type { StaticData } from "../lib/ddragon";
import type { ConnStatus, PastGame, Profile } from "../lib/types";

interface Props {
  profile: Profile | null;
  status: ConnStatus | null;
  staticData: StaticData | null;
  onAbrirPool: () => void;
}

const BR = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });

const LANES: Record<string, string> = {
  TOP: "TOP",
  JUNGLE: "JG",
  MIDDLE: "MID",
  BOTTOM: "ADC",
  UTILITY: "SUP",
  NONE: "—",
};

function quando(ms: number) {
  const horas = (Date.now() - ms) / 3_600_000;
  if (horas < 1) return "agora há pouco";
  if (horas < 24) return `há ${Math.floor(horas)}h`;
  const dias = Math.floor(horas / 24);
  return dias === 1 ? "ontem" : `há ${dias} dias`;
}

function duracao(segundos: number) {
  return `${Math.floor(segundos / 60)} min`;
}

/** KDA como razão. Zero mortes vira "perfeito" em vez de divisão por zero. */
function kda(g: PastGame) {
  if (g.deaths === 0) return "perfeito";
  return ((g.kills + g.assists) / g.deaths).toFixed(1);
}

function Partida({ game, staticData }: { game: PastGame; staticData: StaticData | null }) {
  const champion = staticData?.champions.get(game.championId);
  const minutos = game.duration / 60;

  return (
    <div className={`partida ${game.win ? "venceu" : "perdeu"}`}>
      <span className="face sm">
        {champion && <img src={champion.iconUrl} alt={champion.name} loading="lazy" />}
      </span>

      <div className="partida-quem">
        <b>{champion?.name ?? `#${game.championId}`}</b>
        <small>
          {LANES[game.lane] ?? game.lane} · {quando(game.playedAt)}
        </small>
      </div>

      <div className="partida-kda">
        <b>
          {game.kills}/{game.deaths}/{game.assists}
        </b>
        <small>{kda(game)} KDA</small>
      </div>

      <div className="partida-num">
        <b>{minutos > 0 ? (game.cs / minutos).toFixed(1) : "—"}</b>
        <small>cs/min</small>
      </div>

      {/* O dano vem do histórico do cliente. Durante a partida ele não existe
          em lugar nenhum da API — por isso só aparece aqui. */}
      <div className="partida-num">
        <b>{BR.format(game.damage)}</b>
        <small>dano</small>
      </div>

      <div className="partida-num">
        <b>{duracao(game.duration)}</b>
        <small>{game.win ? "vitória" : "derrota"}</small>
      </div>
    </div>
  );
}

export default function Home({ profile, status, staticData, onAbrirPool }: Props) {
  if (!profile) {
    return (
      <div className="empty">
        <h2>Abra o League of Legends</h2>
        <p>
          {status?.connected
            ? "Cliente conectado. Buscando seu perfil…"
            : "Seu nick, seu rank e suas partidas vêm do próprio cliente — com ele fechado, não há o que mostrar."}
        </p>
      </div>
    );
  }

  const { rank } = profile;
  const partidas = profile.matches;
  const vitorias = partidas.filter((g) => g.win).length;

  return (
    <div className="home">
      <div className="home-head">
        <div>
          <h2>{profile.riotId}</h2>
          <span className="muted">nível {profile.level}</span>
        </div>

        {rank ? (
          <div className="rank">
            <b>
              {rank.tier} {rank.division}
            </b>
            <small>
              {rank.points} PdL · {rank.wins}V {rank.losses}D
              {rank.wins + rank.losses > 0 &&
                ` · ${Math.round((rank.wins / (rank.wins + rank.losses)) * 100)}%`}
            </small>
          </div>
        ) : (
          <div className="rank">
            <b className="muted">sem ranqueada</b>
            <small>esta temporada</small>
          </div>
        )}
      </div>

      {partidas.length > 0 && (
        <p className="home-resumo">
          Nas últimas {partidas.length}: <b className={vitorias * 2 >= partidas.length ? "good" : "bad"}>
            {vitorias}V {partidas.length - vitorias}D
          </b>
        </p>
      )}

      <div className="panel-head">
        <div>
          <h3 className="section-h">Últimas partidas</h3>
        </div>
        <button type="button" className="ghost" onClick={onAbrirPool}>
          Editar minha pool
        </button>
      </div>

      {partidas.length === 0 ? (
        <p className="muted">Nenhuma partida no histórico do cliente.</p>
      ) : (
        <div className="partidas">
          {partidas.map((game) => (
            <Partida key={game.gameId} game={game} staticData={staticData} />
          ))}
        </div>
      )}

      <p className="faint">
        Tudo nesta tela sai do seu próprio cliente, em 127.0.0.1 — nada passa
        pela internet nem por servidor nosso.
      </p>
    </div>
  );
}
