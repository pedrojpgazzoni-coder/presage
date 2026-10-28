import type { MapTimers, Spawn } from "../lib/types";

interface Props {
  upcoming: Spawn[];
  map: MapTimers;
  /** Modo de ajuste: desenha o contorno do minimapa para encaixar. */
  guia?: boolean;
}

/**
 * Posição de cada cova no minimapa, de 0 a 1.
 *
 * A origem é o canto de baixo à esquerda, como no jogo. O minimapa do League
 * **não espelha** quando você está no lado roxo — o Barão fica em cima à
 * esquerda para os dez jogadores —, então uma tabela só serve para os dois
 * lados.
 *
 * Os números são o centro aproximado das covas na Summoner's Rift. Aproximado
 * é suficiente: o relógio tem de cair *em cima* do objetivo, não medir a cova.
 */
const COVAS: Record<string, { x: number; y: number }> = {
  // Barão, Arauto e Grubs dividem o lado de cima do rio.
  "Barão": { x: 0.34, y: 0.7 },
  Arauto: { x: 0.34, y: 0.7 },
  Grubs: { x: 0.3, y: 0.64 },
  // Dragão e Ancião, embaixo à direita.
  "Dragão": { x: 0.66, y: 0.3 },
  "Ancião": { x: 0.66, y: 0.3 },
};

function clock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/**
 * Os relógios de objetivo, em cima do minimapa do jogo.
 *
 * Em pilha num canto eles viram mais uma lista para ler no meio da luta. Em
 * cima da cova, a informação já está onde a pessoa vai olhar de qualquer jeito.
 */
export default function MapaTimers({ upcoming, map, guia = false }: Props) {
  const marcados = upcoming.filter((spawn) => COVAS[spawn.name]);
  if (marcados.length === 0 && !guia) return null;

  return (
    <div
      className={`mapa ${guia ? "guia" : ""}`}
      style={{
        width: map.size,
        height: map.size,
        right: map.right,
        bottom: map.bottom,
      }}
    >
      {marcados.map((spawn) => {
        const cova = COVAS[spawn.name];
        const pronto = spawn.secondsLeft <= 0;

        return (
          <div
            key={spawn.name}
            className={`cova ${pronto ? "pronto" : ""} ${spawn.secondsLeft <= 60 ? "perto" : ""}`}
            style={{ left: `${cova.x * 100}%`, bottom: `${cova.y * 100}%` }}
          >
            <span className="cova-nome">{spawn.name}</span>
            <span className="cova-tempo">
              {pronto ? "pronto" : clock(spawn.secondsLeft)}
              {/* `est` quer dizer que o relógio saiu da constante do patch e
                  não de um abate que vimos acontecer. */}
              {!spawn.observed && <i>est</i>}
            </span>
          </div>
        );
      })}
    </div>
  );
}
