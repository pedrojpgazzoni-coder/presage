import {
  MAPA_LIMITES,
  MAPA_PADRAO,
  OVERLAY_SECTIONS,
  type MapTimers,
  type OverlayPrefs,
  type OverlaySection,
  type Spawn,
} from "../lib/types";
import MapaTimers from "./MapaTimers";

interface Props {
  prefs: OverlayPrefs;
  onChange: (prefs: OverlayPrefs) => void;
}

/** Objetivos de mentira, só para a prévia ter o que desenhar. */
const PREVIA: Spawn[] = [
  { name: "Barão", at: 1500, secondsLeft: 184, observed: false },
  { name: "Dragão", at: 900, secondsLeft: 42, observed: true },
];

/**
 * O que aparece por cima do jogo.
 *
 * Não existe configuração certa aqui. Quem joga de suporte olha objetivo e
 * ignora build; quem joga de ADC quer o próximo item e acha o ouro do time
 * ruído. Por isso cada seção liga e desliga em vez de a gente escolher por
 * todo mundo.
 */
export default function OverlayPrefsPanel({ prefs, onChange }: Props) {
  const mostra = (id: OverlaySection) => !prefs.hidden.includes(id);
  const nenhuma = prefs.hidden.length >= OVERLAY_SECTIONS.length;

  function alternar(id: OverlaySection) {
    const hidden = mostra(id)
      ? [...prefs.hidden, id]
      : prefs.hidden.filter((s) => s !== id);
    onChange({ ...prefs, hidden });
  }

  function ajustarMapa(campo: keyof MapTimers, valor: number) {
    onChange({ ...prefs, map: { ...prefs.map, [campo]: valor } });
  }

  return (
    <div className="prefs">
      <div className="panel-head">
        <div>
          <h2>O que aparece no overlay</h2>
          <span className="muted">
            Vale na hora, mesmo com a partida rodando. O painel continua
            mostrando tudo.
          </span>
        </div>
        {prefs.hidden.length > 0 && (
          <button
            type="button"
            className="ghost"
            onClick={() => onChange({ ...prefs, hidden: [] })}
          >
            Mostrar tudo
          </button>
        )}
      </div>

      <div className="prefs-lista">
        {OVERLAY_SECTIONS.map((secao) => (
          <label className={`pref ${mostra(secao.id) ? "on" : ""}`} key={secao.id}>
            <input
              type="checkbox"
              checked={mostra(secao.id)}
              onChange={() => alternar(secao.id)}
            />
            <span>
              <b>{secao.label}</b>
              <small>{secao.detail}</small>
            </span>
          </label>
        ))}
      </div>

      {nenhuma && (
        <p className="warn-line">
          Tudo desligado: o overlay não vai aparecer. O painel continua
          funcionando normalmente.
        </p>
      )}

      {mostra("objectives") && (
        <div className="mapa-ajuste">
          <div className="panel-head">
            <div>
              <h3 className="section-h">Encaixe no minimapa</h3>
              <p className="faint">
                Os relógios de objetivo ficam <b>em cima do minimapa do jogo</b>,
                na cova do Barão e na do Dragão. O tamanho do minimapa é escolhido
                por você nas opções do League, e não há como o Presage lê-lo — então
                ajuste uma vez aqui e ele fica.
              </p>
            </div>
            {(prefs.map.size !== MAPA_PADRAO.size ||
              prefs.map.right !== MAPA_PADRAO.right ||
              prefs.map.bottom !== MAPA_PADRAO.bottom) && (
              <button
                type="button"
                className="ghost"
                onClick={() => onChange({ ...prefs, map: MAPA_PADRAO })}
              >
                Voltar ao padrão
              </button>
            )}
          </div>

          <div className="mapa-campos">
            <div className="mapa-campo">
              <label htmlFor="mapa-size">
                Tamanho <b>{prefs.map.size}px</b>
              </label>
              <input
                id="mapa-size"
                type="range"
                min={MAPA_LIMITES.minSize}
                max={MAPA_LIMITES.maxSize}
                value={prefs.map.size}
                onChange={(e) => ajustarMapa("size", Number(e.target.value))}
              />
            </div>

            <div className="mapa-campo">
              <label htmlFor="mapa-right">
                Da direita <b>{prefs.map.right}px</b>
              </label>
              <input
                id="mapa-right"
                type="range"
                min={0}
                max={MAPA_LIMITES.maxOffset}
                value={prefs.map.right}
                onChange={(e) => ajustarMapa("right", Number(e.target.value))}
              />
            </div>

            <div className="mapa-campo">
              <label htmlFor="mapa-bottom">
                De baixo <b>{prefs.map.bottom}px</b>
              </label>
              <input
                id="mapa-bottom"
                type="range"
                min={0}
                max={MAPA_LIMITES.maxOffset}
                value={prefs.map.bottom}
                onChange={(e) => ajustarMapa("bottom", Number(e.target.value))}
              />
            </div>
          </div>

          {/* A prévia mostra o arranjo dos relógios, não a medida: o tamanho
              em pixels só faz sentido na sua tela, com o jogo aberto. */}
          <div className="mapa-previa">
            <MapaTimers upcoming={PREVIA} map={prefs.map} guia />
          </div>

          <p className="faint">
            Com a partida aberta, <b>Ctrl+Shift+P</b> liga o overlay e você vê os
            relógios caírem no minimapa de verdade. Mexer nos controles vale na hora.
          </p>
        </div>
      )}

      {/* Esta linha evita uma decepção: quem procura "dano de todos" no
          overlay precisa saber por que ele não está lá. */}
      <p className="warn-line">
        O <b>dano de todos na partida</b> não aparece aqui, e não é escolha
        nossa: durante o jogo a API da Riot não entrega o dano de ninguém, nem o
        seu. O número real está na aba <b>Pós-jogo</b>, assim que a partida
        acaba, vindo do histórico do seu cliente.
      </p>

      <p className="faint">
        O overlay abre e fecha com <b>Ctrl+Shift+P</b>. Ele nasce atravessável
        pelo mouse, então não come clique; <b>Ctrl+Shift+O</b> deixa a janela
        clicável quando você quiser arrastá-la.
      </p>
    </div>
  );
}
