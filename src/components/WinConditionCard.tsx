import type { WinCondition } from "../lib/types";

interface Props {
  wincon: WinCondition | null;
}

/** A janela vira uma posição na barra de três fases. */
const FASES = ["early game", "mid game", "late game"];

export default function WinConditionCard({ wincon }: Props) {
  if (!wincon) return null;

  const forte = wincon.confidence >= 0.5;

  return (
    <section className="wincon">
      <div className="panel-head">
        <div>
          <h3 className="section-h">Win condition</h3>
          <p className="faint">
            {wincon.fromData
              ? "Sai da base de partidas do patch."
              : "Sai do modelo de combate — atributos e itens, não partidas agregadas."}
          </p>
        </div>
      </div>

      <p className={`wincon-head ${forte ? "on" : ""}`}>{wincon.headline}</p>

      {/* A barra diz a mesma coisa da frase, mas de relance. */}
      <div className="wincon-fases">
        {FASES.map((fase) => (
          <span
            key={fase}
            className={fase === wincon.window ? "fase on" : "fase"}
            style={fase === wincon.window ? { opacity: 0.45 + wincon.confidence * 0.55 } : undefined}
          >
            {fase}
          </span>
        ))}
      </div>

      <div className="wincon-listas">
        {wincon.play.length > 0 && (
          <div className="wincon-lista">
            <h4 className="good">Façam</h4>
            <ul>
              {wincon.play.map((linha) => (
                <li key={linha}>{linha}</li>
              ))}
            </ul>
          </div>
        )}

        {wincon.avoid.length > 0 && (
          <div className="wincon-lista">
            <h4 className="bad">Evitem</h4>
            <ul>
              {wincon.avoid.map((linha) => (
                <li key={linha}>{linha}</li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </section>
  );
}
