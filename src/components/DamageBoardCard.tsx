import type { StaticData } from "../lib/ddragon";
import type { DamageBoard, DamageRow } from "../lib/types";

interface Props {
  board: DamageBoard | null;
  staticData: StaticData | null;
}

const BR = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });

function porMinuto(dano: number, duracao: number) {
  const minutos = duracao / 60;
  return minutos > 0 ? dano / minutos : 0;
}

function Linha({
  row,
  maior,
  duracao,
  staticData,
}: {
  row: DamageRow;
  maior: number;
  duracao: number;
  staticData: StaticData | null;
}) {
  const champion = staticData?.champions.get(row.championId);
  // A barra é relativa ao maior dano da partida, não ao do time: quem bateu
  // mais bateu mais, e o desenho tem de dizer isso de um lado só.
  const largura = maior > 0 ? (row.damage / maior) * 100 : 0;

  return (
    <div className={`dano-linha ${row.ally ? "aliado" : "inimigo"} ${row.isMe ? "eu" : ""}`}>
      <span className="face sm">
        {champion && <img src={champion.iconUrl} alt={champion.name} loading="lazy" />}
      </span>

      <div className="dano-quem">
        <b>{champion?.name ?? `#${row.championId}`}</b>
        <small>{row.isMe ? "você" : row.riotId || "—"}</small>
      </div>

      <div className="dano-kda">
        {row.kills}/{row.deaths}/{row.assists}
      </div>

      <div className="dano-barra">
        <span style={{ width: `${largura}%` }} />
      </div>

      <div className="dano-num">
        <b>{BR.format(row.damage)}</b>
        <small>{BR.format(porMinuto(row.damage, duracao))}/min</small>
      </div>

      <div className="dano-num">
        <b>{Math.round(row.teamShare * 100)}%</b>
        <small>do time</small>
      </div>
    </div>
  );
}

/**
 * O dano de todos na partida que acabou.
 *
 * Este placar é pós-partida porque **não existe outro jeito**. A Live Client
 * Data API, que é a única fonte legal durante o jogo, entrega abates, tropas e
 * visão — e nada de dano, nem o seu. Os números aqui são os verdadeiros, do
 * histórico do próprio cliente, não uma estimativa.
 */
export default function DamageBoardCard({ board, staticData }: Props) {
  if (!board || board.rows.length === 0) return null;

  const maior = Math.max(...board.rows.map((r) => r.damage));
  const minutos = Math.round(board.duration / 60);

  return (
    <section className="dano">
      <div className="panel-head">
        <div>
          <h3 className="section-h">Dano de todos na partida</h3>
          <p className="faint">
            Números reais do histórico do seu cliente, não estimativa. O dano só
            existe depois do apito: durante a partida a API da Riot não entrega
            o dano de ninguém.
          </p>
        </div>
        <div className="rank">
          <b className={board.win ? "good" : "bad"}>
            {board.win ? "Vitória" : "Derrota"}
          </b>
          <small>{minutos} min</small>
        </div>
      </div>

      <div className="dano-times">
        <span>
          Seu time <b>{BR.format(board.allyTotal)}</b>
        </span>
        <span className="muted">x</span>
        <span>
          <b>{BR.format(board.enemyTotal)}</b> eles
        </span>
      </div>

      <div className="dano-lista">
        {board.rows.map((row) => (
          <Linha
            key={`${row.teamId}-${row.championId}-${row.riotId}`}
            row={row}
            maior={maior}
            duracao={board.duration}
            staticData={staticData}
          />
        ))}
      </div>
    </section>
  );
}
