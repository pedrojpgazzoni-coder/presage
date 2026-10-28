# Instalador do Presage

O `.exe` da versão atual, versionado junto com o código.

## O que tem aqui

| arquivo | o que é |
|---|---|
| `Presage_0.9.0_x64-setup.exe` | o instalador para Windows |
| `Presage_0.9.0_x64-setup.exe.sig` | a assinatura do atualizador para esse arquivo |
| `latest.json` | o manifesto que o app lê para saber se há versão nova |

## Como instalar

Dois cliques no `.exe`. Ele instala em `%LOCALAPPDATA%\Presage` e não pede
permissão de administrador.

O Windows vai mostrar um aviso de editor desconhecido, porque o binário ainda
não é assinado com certificado de código. Isso é esperado enquanto o
certificado não existir — não é um problema do arquivo.

## Sobre esta pasta

Instalador dentro do repositório é exceção, não o padrão. O `.gitignore` barra
`*.exe` justamente porque cada versão nova soma ~2,5 MB ao histórico e o git
nunca esquece: tirar depois exige reescrever o histórico inteiro.

Por isso aqui fica **uma versão por vez**. Ao subir uma versão, troque os três
arquivos em vez de acumular — senão o repositório cresce de forma que não volta
atrás.

## Para a atualização automática

Esta pasta é conveniência para quem clona o repositório. **O app não se atualiza
por ela.** O updater lê

```
https://github.com/pedrojpgazzoni-coder/presage/releases/latest/download/latest.json
```

ou seja, a release publicada no GitHub. Para a atualização automática funcionar,
o `latest.json` e o `.exe` precisam estar anexados a uma release **publicada**
(rascunho não é servido por esse endereço), e o workflow precisa do secret
`UPDATER_PRIVATE_KEY` para assinar.
