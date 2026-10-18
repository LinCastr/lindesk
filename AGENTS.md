# AGENTS.md — regras gerais para agentes de IA

Contrato de trabalho que vale para **qualquer projeto**. O que é específico deste
projeto fica em `PROJETO.md`; o que já foi feito, os problemas resolvidos e o
roadmap ficam em `RESUMO.md`. Leia os dois antes da primeira edição. Se algum
não existir, crie ao fim da primeira tarefa (`PROJETO.md`: como verificar,
regras e armadilhas do projeto; `RESUMO.md`: feito, resolvido, falta testar,
roadmap — e ponha no `.gitignore` se o repositório for público).

@PROJETO.md
@RESUMO.md

Responda sempre em **português do Brasil**.

---

## 1. Como trabalhar

A maioria dos bugs nasce de edição apressada, não de tarefa difícil.

1. **Leia antes de escrever.** Abra o arquivo que vai mudar e a parte da
   documentação que fala dele. Muito do que parece bug já está explicado lá.
2. **Diga o diagnóstico antes de editar.** Em 3–6 linhas: o que entendeu do
   pedido, onde está a causa (arquivo e linha) e o que pretende mudar. Se o
   pedido tiver duas leituras possíveis, **pergunte** em vez de adivinhar.
3. **Escopo mínimo.** Mexa só no que o pedido cobre. Nada de renomear,
   reordenar, reformatar ou "melhorar de passagem".
4. **Uma mudança por vez.** Edite, verifique, só então siga para a próxima.
5. **Verifique de verdade** (como, está no `PROJETO.md`). Sem verificação, não
   diga que está pronto.
6. **Relate com honestidade:** o que mudou, o que testou e o que **não** testou.
   Nunca escreva "corrigido e testado" sobre algo que só leu.

Se algo não estiver claro no meio da tarefa, pare e pergunte.

## 2. Economia de tokens

- **Comece pelo `RESUMO.md`**, não pelo histórico da conversa nem por documentos
  longos. Ele existe para a sessão nova não refazer investigação.
- **Não leia arquivo grande inteiro.** Use `grep -n` para achar a seção e leia só
  o trecho (`sed -n 'A,Bp'`, ou leitura com offset/limit).
- **Não releia** o que já está no contexto nem o arquivo que acabou de editar.
- Saída de comando: filtre (`| tail`, `| grep`, `| head`) em vez de despejar tudo.
- **Ao terminar cada tarefa, atualize o `RESUMO.md`:** 1–3 linhas no histórico
  (o que foi feito, causa do problema e como foi resolvido) e o roadmap. Curto:
  o detalhe vai na documentação detalhada do projeto, se houver.

## 3. Segurança e git

- **Idioma do repositório: inglês.** Tudo que vai para o GitHub (README e docs
  públicas, mensagens de commit, títulos e descrições de PR/issue, comentários
  e textos do código) sai em inglês. Português só se o usuário pedir. A conversa
  com o usuário continua em português do Brasil.
- **Não faça `git push`** sem o usuário pedir. Commit, sim; publicar, só a pedido.
- **Novos repositórios na organização:** todo repositório novo deve ser criado na organização **LinCastr** (`github.com/LinCastr`), nunca na conta pessoal.
- **Nunca comite** dados pessoais, tokens, senhas, histórico, nem caminhos com o
  nome do usuário (`/home/<usuário>`).
- **Ações que podem impedir o boot ou o login** (driver, bootloader, gerenciador
  de login, serviços do sistema): avise antes, ofereça teste e peça confirmação.
- **Não mate processo por padrão** (`pkill -f`): pode casar com o próprio shell do
  agente. Mate por PID (`pgrep -f` também casa com a linha do próprio comando: use
  `pgrep -x nome` e confira `/proc/PID/exe`).
- **Teste de janela/GUI nunca na workspace do usuário** (ele pode estar jogando ou
  trabalhando). Abra numa workspace separada, sem trocar o foco:
  `hyprctl dispatch "hl.dsp.exec_cmd('[workspace 9 silent] /caminho/script.sh')"`
  (Hyprland 0.56 usa Lua; ponha o comando num script), e confirme com
  `hyprctl clients -j` que a janela foi para lá. Mesmo assim, só teste GUI quando
  precisar; prefira testes headless/CLI. Atenção: o app reiniciado pelo atualizador
  (`exec`) abre janela nova na workspace **ativa**, então não teste o reinício do
  atualizador com a janela real; use o teste local com servidor falso só até a troca
  do binário.
- Antes de apagar ou sobrescrever, olhe o alvo.

## 4. Comunicação

- Avise só antes dos passos importantes; não narre cada micro-passo.
- Se um comando, teste ou build falhar, relate todos os erros de forma objetiva.
- Comandos para o usuário rodar: um por bloco ```bash, sem `$` na frente.

## 5. Planejamento

- **Tarefa complexa ou que toca vários arquivos:** apresente o plano (escopo,
  passos, arquivos) e espere o OK.
- **Ajuste trivial e isolado:** pode ir direto.

## 6. Mais de um agente

Se houver um `HANDOFF.md`: leia antes de começar, registre os arquivos que vai
ocupar, não mexa em arquivo ocupado pelo outro agente e, ao terminar, comite e
anote o que fez e o que não testou. Bug achado no trabalho do outro vai para a
seção "Bugs encontrados" do `HANDOFF.md`, não é corrigido direto.
