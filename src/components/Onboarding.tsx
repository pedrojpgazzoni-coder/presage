import type { StaticData } from "../lib/ddragon";
import type { Pool } from "../lib/types";
import Logo from "./Logo";
import PoolBuilder from "./PoolBuilder";

interface Props {
  pool: Pool;
  staticData: StaticData | null;
  onChange: (pool: Pool) => void;
  onSuggest?: Parameters<typeof PoolBuilder>[0]["onSuggest"];
  onPronto: () => void;
}

/**
 * A primeira vez que o app abre.
 *
 * Não é uma tela separada: é o próprio Construtor de Pool com uma abertura.
 * Duplicar o fluxo criaria duas telas que fazem a mesma pergunta e podem
 * divergir — e a pessoa teria de responder de novo depois.
 */
export default function Onboarding({
  pool,
  staticData,
  onChange,
  onSuggest,
  onPronto,
}: Props) {
  const pronto = pool.entries.length > 0;

  return (
    <div className="onboarding">
      <div className="onboarding-abertura">
        <Logo size={44} />
        <h1>Vamos começar pela sua pool.</h1>
        <p>
          Diga a rota que você joga e os campeões que você pega. É com isso que
          o Presage ranqueia os picks a seu favor em vez de dar a resposta
          média do patch.
        </p>
        <p className="faint">
          Só desta vez. Depois a pool fica no menu e você muda quando quiser.
        </p>
      </div>

      <PoolBuilder
        pool={pool}
        staticData={staticData}
        onChange={onChange}
        onSuggest={onSuggest}
      />

      <div className="onboarding-pe">
        <button
          type="button"
          className="primary"
          onClick={onPronto}
          disabled={!pronto}
        >
          {pronto ? "Pronto, começar a usar" : "Escolha ao menos um campeão"}
        </button>
        {/* Ninguém fica preso aqui: a pool pode ser montada depois, e um app
            que exige configuração antes de abrir é um app que não abre. */}
        <button type="button" className="ghost" onClick={onPronto}>
          Fazer isso depois
        </button>
      </div>
    </div>
  );
}
