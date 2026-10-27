import { useEffect, useMemo, useState } from "react";
import type { StaticData } from "../lib/ddragon";
import {
  GAP_TEXTO,
  ROLES,
  TIERS,
  type Gap,
  type Pool,
  type PoolCandidate,
  type Role,
  type Tier,
} from "../lib/types";

interface Props {
  pool: Pool;
  staticData: StaticData | null;
  onChange: (pool: Pool) => void;
  /** Busca o que falta na pool desta rota. Ausente na demonstração. */
  onSuggest?: (role: Role | null) => Promise<[Gap[], PoolCandidate[]]>;
}

/** Quantos o passo pede. Enquanto o passo está aberto, ele não deixa passar
 *  disso — mas dá para continuar com menos, e na revisão não há limite. */
const ALVO: Record<Exclude<Tier, "extra">, number> = { core: 2, situational: 3 };

type Passo = "rota" | "core" | "situational" | "fechar" | "revisao";

export default function PoolBuilder({ pool, staticData, onChange, onSuggest }: Props) {
  // Abrir o Pool sempre começa pela rota, com ou sem pool montada: é a
  // pergunta que organiza todo o resto, e os cartões já mostram o que cada
  // rota tem.
  const [role, setRole] = useState<Role | null>(null);
  const [passo, setPasso] = useState<Passo>("rota");
  const [search, setSearch] = useState("");
  const [buracos, setBuracos] = useState<Gap[]>([]);
  const [candidatos, setCandidatos] = useState<PoolCandidate[] | null>(null);

  // As recomendações dependem dos cinco já escolhidos, então só fazem sentido
  // quando o passo abre — e se refazem se a pessoa voltar e trocar alguém.
  useEffect(() => {
    if (passo !== "fechar" || !onSuggest) return;
    let vivo = true;
    setCandidatos(null);
    onSuggest(role)
      .then(([gaps, lista]) => {
        if (!vivo) return;
        setBuracos(gaps);
        setCandidatos(lista);
      })
      .catch(() => vivo && setCandidatos([]));
    return () => {
      vivo = false;
    };
  }, [passo, role, onSuggest, pool]);

  const entries = pool.entries.filter((entry) => entry.role === role);
  const chosen = new Set(entries.map((entry) => entry.championId));
  const conta = (tier: Tier) => entries.filter((entry) => entry.tier === tier).length;

  const catalog = useMemo(() => {
    if (!staticData) return [];
    const term = search.trim().toLowerCase();
    return [...staticData.champions.values()]
      .filter((champion) => !term || champion.name.toLowerCase().includes(term))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [staticData, search]);

  function add(championId: number, tier: Tier) {
    if (chosen.has(championId)) return;
    onChange({ entries: [...pool.entries, { championId, role, tier }] });
  }

  function retier(championId: number, tier: Tier) {
    onChange({
      entries: pool.entries.map((entry) =>
        entry.championId === championId && entry.role === role ? { ...entry, tier } : entry
      ),
    });
  }

  function remove(championId: number) {
    onChange({
      entries: pool.entries.filter(
        (entry) => !(entry.championId === championId && entry.role === role)
      ),
    });
  }

  function escolherRota(proxima: Role) {
    setRole(proxima);
    setSearch("");
    // Sempre pelos passos, mesmo com a rota já montada.
    //
    // Antes, uma rota com um campeão só já pulava para a revisão — e aí os
    // três passos ficavam inalcançáveis para sempre, porque todo mundo
    // escolhe o primeiro campeão em algum momento. Os passos mostram o que já
    // está escolhido nos lugares, então revisitar é barato.
    setPasso("core");
  }

  // ---- passo 1: a rota ----

  if (passo === "rota") {
    return (
      <div className="builder">
        <div className="wizard-head">
          <h2>Em que rota você joga?</h2>
          <p className="muted">
            A pool é por rota. Dá para montar as outras depois.
          </p>
        </div>
        <div className="role-cards">
          {ROLES.map((item) => {
            const daRota = pool.entries.filter((entry) => entry.role === item.id);
            const conta = (tier: Tier) => daRota.filter((e) => e.tier === tier).length;
            const completa = conta("core") >= ALVO.core && conta("situational") >= ALVO.situational;

            return (
              <button
                type="button"
                key={item.id}
                className={daRota.length > 0 ? "com-pool" : ""}
                onClick={() => escolherRota(item.id)}
              >
                <b>{item.label}</b>
                {/* Quantos em cada tier, e não só o total: "1 escolhidos" não
                    diz se falta core ou situacional. */}
                <small>
                  {daRota.length === 0
                    ? "vazia"
                    : `${conta("core")}/${ALVO.core} core · ${conta("situational")}/${ALVO.situational} situacionais`}
                </small>
                {completa && <em>pronta</em>}
              </button>
            );
          })}
        </div>
      </div>
    );
  }

  // ---- passos 2 e 3: escolher os campeões ----

  if (passo === "core" || passo === "situational") {
    const alvo = ALVO[passo];
    const feitos = conta(passo);
    const cheio = feitos >= alvo;
    const rotulo = ROLES.find((item) => item.id === role)?.label ?? "";
    const escolhidos = entries.filter((entry) => entry.tier === passo);

    return (
      <div className="builder">
        <div className="wizard-head">
          <p className="eyebrow">
            {rotulo} · passo {passo === "core" ? 2 : 3} de 4
          </p>
          <h2>
            {passo === "core"
              ? `Escolha seus ${alvo} campeões core`
              : `Escolha seus ${alvo} campeões situacionais`}
          </h2>
          <p className="muted">
            {passo === "core"
              ? "Os que você pega em quase todo jogo."
              : "Os que entram contra uma composição específica."}
          </p>
        </div>

        <div className="wizard-track">
          {Array.from({ length: alvo }, (_, i) => {
            const entry = escolhidos[i];
            const champion = entry && staticData?.champions.get(entry.championId);
            return (
              <div className={`wizard-slot ${entry ? "on" : ""}`} key={i}>
                {champion ? (
                  <>
                    <img src={champion.iconUrl} alt={champion.name} />
                    <button
                      type="button"
                      className="remove"
                      onClick={() => remove(entry.championId)}
                      title={`Tirar ${champion.name}`}
                    >
                      ×
                    </button>
                  </>
                ) : (
                  <span>{i + 1}</span>
                )}
              </div>
            );
          })}
          <span className="counter">
            {feitos} de {alvo}
          </span>
        </div>

        <input
          className="search"
          type="search"
          placeholder="Buscar campeão…"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          autoFocus
        />

        <div className="grid grid-catalog">
          {catalog.map((champion) => {
            const pego = chosen.has(champion.id);
            return (
              <button
                type="button"
                key={champion.id}
                className={`cell ${pego ? "cell-taken" : ""}`}
                onClick={() => add(champion.id, passo)}
                title={cheio && !pego ? `Já escolheu ${alvo}` : champion.name}
                disabled={pego || cheio}
              >
                <div className="cell-portrait">
                  <img src={champion.iconUrl} alt={champion.name} loading="lazy" />
                </div>
                <span className="cell-name">{champion.name}</span>
              </button>
            );
          })}
        </div>

        <div className="wizard-nav">
          <button
            type="button"
            className="ghost"
            onClick={() => {
              setSearch("");
              setPasso(passo === "core" ? "rota" : "core");
            }}
          >
            Voltar
          </button>
          <button
            type="button"
            className="primary"
            onClick={() => {
              setSearch("");
              setPasso(passo === "core" ? "situational" : "fechar");
            }}
          >
            {cheio ? "Continuar" : feitos === 0 ? "Pular" : `Continuar com ${feitos}`}
          </button>
        </div>
      </div>
    );
  }

  // ---- passo 4: o último, escolhido pelo que falta ----

  if (passo === "fechar") {
    const rotulo = ROLES.find((item) => item.id === role)?.label ?? "";

    return (
      <div className="builder">
        <div className="wizard-head">
          <p className="eyebrow">{rotulo} · passo 4 de 4</p>
          <h2>Escolha um destes</h2>
          {buracos.length > 0 ? (
            <p className="muted">
              Pelos cinco que você escolheu: {buracos.map((g) => GAP_TEXTO[g]).join(" · ")}.
            </p>
          ) : (
            <p className="muted">
              Escolha um a mais, se quiser. Sua pool já cobre tipo de dano,
              frontline e alcance.
            </p>
          )}
        </div>

        {candidatos === null ? (
          <p className="muted">Calculando…</p>
        ) : candidatos.length === 0 ? (
          <p className="muted">
            Nada a recomendar — e isso é um bom sinal. Dá para fechar aqui.
          </p>
        ) : (
          <div className="grid grid-catalog grid-sugestao">
            {candidatos.map((c) => {
              const champion = staticData?.champions.get(c.championId);
              return (
                <button
                  type="button"
                  key={c.championId}
                  className="cell cell-sugestao"
                  onClick={() => {
                    add(c.championId, "extra");
                    setPasso("revisao");
                  }}
                  title={c.reasons.join(" · ")}
                >
                  <div className="cell-portrait">
                    {champion && (
                      <img src={champion.iconUrl} alt={champion.name} loading="lazy" />
                    )}
                  </div>
                  <span className="cell-name">{champion?.name ?? c.name}</span>
                  <span className="cell-why">{c.reasons[0]}</span>
                </button>
              );
            })}
          </div>
        )}

        {/* O número que aparece ao lado não é winrate, e dizer isso é mais
            importante que o número. */}
        <p className="faint">
          Esta lista sai da composição da sua pool e da matemática de
          resistências do jogo — que tipo de dano falta, quem segura a linha de
          frente, quem varia menos conforme o dano que vem. Não sai de winrate:
          essa base ainda não existe.
        </p>

        <div className="wizard-nav">
          <button
            type="button"
            className="ghost"
            onClick={() => setPasso("situational")}
          >
            Voltar
          </button>
          <button type="button" className="primary" onClick={() => setPasso("revisao")}>
            Fechar sem o sexto
          </button>
        </div>
      </div>
    );
  }

  // ---- passo 4: revisão, sem limite ----

  return (
    <div className="builder">
      <div className="builder-head">
        <div>
          <h2>Sua pool</h2>
          <span className="muted">Aqui não há limite. Ajuste o que quiser.</span>
        </div>
        <button type="button" className="ghost" onClick={() => setPasso("rota")}>
          Outra rota
        </button>
      </div>

      <div className="role-tabs">
        {ROLES.map((item) => (
          <button
            type="button"
            key={item.id}
            className={role === item.id ? "on" : ""}
            onClick={() => setRole(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>

      {TIERS.map((tier) => {
        const group = entries.filter((entry) => entry.tier === tier.id);
        return (
          <div className="section" key={tier.id}>
            <h3>
              {tier.label} <span className="muted">({group.length})</span>
            </h3>
            {group.length === 0 ? (
              <p className="muted">Vazio.</p>
            ) : (
              <div className="grid grid-pool">
                {group.map((entry) => {
                  const champion = staticData?.champions.get(entry.championId);
                  return (
                    <div className="cell cell-static" key={entry.championId}>
                      <div className="cell-portrait">
                        {champion ? (
                          <img src={champion.iconUrl} alt={champion.name} loading="lazy" />
                        ) : (
                          <span>?</span>
                        )}
                      </div>
                      <span className="cell-name">{champion?.name ?? `#${entry.championId}`}</span>
                      <div className="cell-actions">
                        {TIERS.map((option) => (
                          <button
                            type="button"
                            key={option.id}
                            className={entry.tier === option.id ? "on" : ""}
                            onClick={() => retier(entry.championId, option.id)}
                            title={option.label}
                          >
                            {option.label[0]}
                          </button>
                        ))}
                        <button
                          type="button"
                          className="remove"
                          onClick={() => remove(entry.championId)}
                          title="Remover"
                        >
                          ×
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}

      <div className="section">
        <h3>Adicionar</h3>
        <input
          className="search"
          type="search"
          placeholder="Buscar campeão…"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <div className="grid grid-catalog">
          {catalog.map((champion) => (
            <button
              type="button"
              key={champion.id}
              className={`cell ${chosen.has(champion.id) ? "cell-taken" : ""}`}
              onClick={() => add(champion.id, "extra")}
              title={champion.name}
              disabled={chosen.has(champion.id)}
            >
              <div className="cell-portrait">
                <img src={champion.iconUrl} alt={champion.name} loading="lazy" />
              </div>
              <span className="cell-name">{champion.name}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
