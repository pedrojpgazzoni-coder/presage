import type { StaticData } from "../lib/ddragon";
import type { LaneReport, LaneOutlook, LevelOdds, Skirmish } from "../lib/types";
import WinConditionCard from "./WinConditionCard";

interface Props {
  report: LaneReport | null;
  staticData: StaticData | null;
}

function pct(value: number) {
  return `${Math.round(value * 100)}%`;
}

function tone(odds: number) {
  if (odds >= 0.55) return "good";
  if (odds <= 0.45) return "bad";
  return "even";
}

/** A chance nas três fases do jogo, com os dois lados no mesmo nível. */
function Niveis({ pontos, grande = false }: { pontos: LevelOdds[]; grande?: boolean }) {
  if (!pontos || pontos.length === 0) return null;
  return (
    <div className={`niveis ${grande ? "niveis-grande" : ""}`}>
      {pontos.map((p, i) => (
        <span key={p.level} className={tone(p.odds)}>
          {/* O nome da fase é como se pensa o jogo; o nível fica junto para o
              rótulo não virar vago. A última leva "+": late game não acaba no
              11, começa nele. */}
          <small>
            {p.phase}{" "}
            <i>
              nv {p.level}
              {i === pontos.length - 1 ? "+" : ""}
            </i>
          </small>
          {pct(p.odds)}
        </span>
      ))}
    </div>
  );
}

function Face({ id, staticData }: { id: number; staticData: StaticData | null }) {
  const champion = staticData?.champions.get(id);
  return (
    <span className="face sm">
      {champion ? <img src={champion.iconUrl} alt={champion.name} loading="lazy" /> : null}
    </span>
  );
}

function LaneRow({ lane, staticData }: { lane: LaneOutlook; staticData: StaticData | null }) {
  return (
    <div className="lane-row">
      <span className="lane-tag">{lane.lane}</span>
      <Face id={lane.ally} staticData={staticData} />
      <span className="lane-vs">x</span>
      <Face id={lane.enemy} staticData={staticData} />

      <div className="lane-bar">
        <span style={{ width: pct(lane.winProbability) }} className={tone(lane.winProbability)} />
      </div>

      <b className={`lane-pct ${tone(lane.winProbability)}`}>{pct(lane.winProbability)}</b>

      <ul className="lane-why">
        {lane.reasons.map((reason) => (
          <li key={reason}>{reason}</li>
        ))}
        {!lane.fromData && <li className="faint">sem base de partidas: só a conta do duelo</li>}
      </ul>

      <div className="lane-niveis">
        <Niveis pontos={lane.byLevel} />
      </div>
    </div>
  );
}

function SkirmishRow({ fight, staticData }: { fight: Skirmish; staticData: StaticData | null }) {
  const meu = Math.round(fight.odds * 100);

  return (
    <div className="fight-row">
      <div className="fight-head">
        <span className="fight-label">{fight.label}</span>
      </div>
      <div className="fight-faces">
        {fight.ours.map((id) => (
          <Face key={`a${id}`} id={id} staticData={staticData} />
        ))}
        <span className="lane-vs">x</span>
        {fight.theirs.map((id) => (
          <Face key={`e${id}`} id={id} staticData={staticData} />
        ))}
      </div>

      {/* Os dois lados escritos: "45%" sozinho não diz de quem é. */}
      <div className="lane-bar">
        <span style={{ width: `${meu}%` }} className={tone(fight.odds)} />
      </div>
      <div className="fight-lados">
        <b className={tone(fight.odds)}>Vocês {meu}%</b>
        <span>Eles {100 - meu}%</span>
      </div>

      <Niveis pontos={fight.byLevel} />

      <p className="fight-detail">
        {fight.detail}
        {fight.reasons.length > 0 && ` · ${fight.reasons.join(" · ")}`}
      </p>
    </div>
  );
}

export default function LaneAnalysis({ report, staticData }: Props) {
  if (!report || report.lanes.length === 0) {
    return (
      <div className="empty">
        <h2>Sem rotas para comparar</h2>
        <p>A análise aparece quando os dois lados tiverem campeões escolhidos.</p>
      </div>
    );
  }

  return (
    <div className="lanes">
      {/* A win condition vem primeiro: é a leitura do jogo. As rotas e as
          escaramuças abaixo são a conta que a sustenta. */}
      <WinConditionCard wincon={report.winCondition} />

      <div className="panel-head">
        <div>
          <h2>Chance de você ganhar cada rota</h2>
          <span className="muted">
            {report.source === "partida"
              ? "níveis e itens reais da partida"
              : "projeção no nível 6, itens iguais"}
          </span>
        </div>
      </div>

      {report.rolesAssumed && (
        <p className="warn-line">
          O cliente não informou as rotas deles. Os pares saíram da ordem do draft.
        </p>
      )}

      {report.team && (
        <div className="time-geral">
          <div className="time-head">
            <span className="label">Os cinco contra os cinco</span>
            <b className={tone(report.team.odds)}>{pct(report.team.odds)}</b>
          </div>
          <div className="time-faces">
            {report.team.ours.map((id) => (
              <Face key={`t${id}`} id={id} staticData={staticData} />
            ))}
            <span className="lane-vs">x</span>
            {report.team.theirs.map((id) => (
              <Face key={`e${id}`} id={id} staticData={staticData} />
            ))}
          </div>
          <Niveis pontos={report.team.byLevel} grande />
          {report.team.reasons.length > 0 && (
            <ul className="lane-why">
              {report.team.reasons.map((r) => (
                <li key={r}>{r}</li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="lane-list">
        {report.lanes.map((lane) => (
          <LaneRow key={lane.lane} lane={lane} staticData={staticData} />
        ))}
      </div>

      {/* A ressalva que não pode faltar: a curva é de atributos, não de
          habilidades. O motor não sabe que alguém ganhou a suprema no 6. */}
      <p className="faint">
        <b>Early · mid · late</b> comparam os dois lados no mesmo nível (3, 6 e
        11), com os itens que cada um tem. Elas mostram o crescimento de vida,
        armadura, resistência e dano por nível — não as habilidades: o motor
        não sabe que alguém ganha a suprema no 6.
      </p>

      {report.skirmishes.length > 0 && (
        <>
          <h3 className="section-h">Se a briga acontecer</h3>
          <div className="fight-list">
            {report.skirmishes.map((fight) => (
              <SkirmishRow key={fight.label} fight={fight} staticData={staticData} />
            ))}
          </div>
        </>
      )}
    </div>
  );
}
