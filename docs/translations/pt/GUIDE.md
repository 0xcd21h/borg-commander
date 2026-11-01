# Guia do utilizador do Borg

[Voltar à apresentação](../../../README.md)

[Instalação](#install) · [Temas e ícones](#themes-and-icons) · [Teclas](#keys) ·
[Rato e consola](#mouse-and-console) · [Plugin Git](#git-plugin) ·
[Localizar, tarefas e visualizadores](#find-jobs-and-viewers) · [Configuração](#configuration)

## Instalação

Transfira um binário a partir das [GitHub Releases](https://github.com/0xcd21h/borg-commander/releases)
ou execute o instalador. Funciona em macOS e Linux, tanto em Intel como em ARM:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

O instalador verifica a soma de controlo do arquivo e coloca `borg` numa pasta do seu PATH, para
que o possa executar de imediato no mesmo terminal. Escolhe `~/.local/bin` se essa pasta já
estiver no PATH; caso contrário, `/usr/local/bin` se tiver permissão de escrita (por exemplo,
como root num contentor); se nenhuma servir, usa `~/.local/bin` e acrescenta-a ao ficheiro de
arranque da sua shell. No fim, inicia o borg; defina `BORG_NO_RUN=1` para o impedir, ou
`BORG_INSTALL_DIR` para escolher a pasta. Se a mesma versão já estiver instalada, voltar a
executar o instalador dispensa a transferência.

Para compilar a partir do código-fonte, precisa do Rust 1.97 ou mais recente:

```sh
cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

O suporte para Homebrew já está preparado e passará a funcionar quando o tap for publicado:

```sh
brew install 0xcd21h/tap/borg-commander
```

Execute `borg` na pasta que pretende explorar. F10 sai e Ctrl-O passa da consola para os painéis.
Para atualizar, execute `borg update` numa cópia transferida, ou use o Homebrew ou o cargo se
tiver instalado dessa forma. `borg --version` mostra a versão instalada.

## Temas e ícones

Escolha um tema em F9 → Opções → Tema. A [galeria de temas](../../../README.md#themes) mostra os
seis: Midnight, Aurora, Neon, Classic, Paper e Ember.

Os temas usam cores fixas, pelo que uma paleta personalizada no terminal não altera o seu aspeto.
Nos terminais sem cor verdadeira (true color), são usadas as cores mais próximas da paleta de 256
cores.

Todos os ficheiros têm um ícone. No Ghostty, o Borg usa automaticamente os ícones mais ricos da
Nerd Font; nos restantes terminais, usa por omissão símbolos portáveis: `/` para pastas, `@` para
ligações e `*` para executáveis. Defina `icon-style = "nerd"` para usar os ícones ricos noutro
terminal com um tipo de letra adequado, ou `icon-style = "portable"` para usar sempre os símbolos
portáveis. O valor por omissão, `"auto"`, adapta-se ao terminal e não guarda o que deteta como
preferência de tipo de letra.

Os arquivos de cada versão incluem o **Borg Mono**, um tipo de letra monoespaçado com os ícones
da Nerd Font. Instale os estilos normal e negrito a partir de `fonts/`, selecione o Borg Mono nas
definições do terminal e escolha o estilo de ícones
`"nerd"`. O Borg não consegue mudar o tipo de letra do terminal por si.
O instalador coloca os tipos de letra em `${XDG_DATA_HOME:-~/.local/share}/borg-commander/fonts`;
o Homebrew coloca-os em `$(brew --prefix)/share/borg-commander/fonts`.

## Coleções e pesquisa recursiva

O menu Pesquisar reúne as coleções e a pesquisa recursiva por nome ou conteúdo de ficheiro. Ambas
mostram os resultados numa lista ordenável com o caminho de origem de cada ficheiro, e ambas
funcionam com o visualizador e com a pré-visualização.

- **Ctrl-F:** pesquisa a pasta atual e tudo o que está abaixo dela. Todas as palavras têm de
  corresponder: `main rs` encontra `main.rs`, e `a | b` encontra qualquer uma das palavras. Uma
  palavra em minúsculas ignora maiúsculas e minúsculas; uma palavra com alguma maiúscula tem de
  corresponder exatamente. Pode também usar padrões glob como `*.log`, expressões entre aspas,
  caminhos com uma barra como `src/lib` e `-word` para excluir correspondências, além dos filtros
  `ext:rs,toml`, `type:file`, `type:dir`, `type:link`, `size:>10M`, `size:1M..50M`, `mod:<7d`
  (alterado na última semana), `mod:>1y` e `in:src` (uma pasta em qualquer ponto do caminho). As
  pesquisas recentes aparecem por baixo do formulário, com as feitas na mesma pasta em primeiro
  lugar; clique numa delas, ou prima Ctrl-P e Ctrl-N no campo de pesquisa, para a reutilizar. Os
  nomes correspondem em qualquer alfabeto, seja qual for a capitalização de um glob, e as letras
  acentuadas correspondem independentemente da forma como o sistema de ficheiros as guarda. As
  correspondências próximas da pasta de pesquisa aparecem primeiro, enquanto o resto da árvore
  ainda está a ser lido.
- **Alt-K:** junta os ficheiros marcados, ou o ficheiro atual, numa coleção no outro painel. Uma
  coleção guarda referências, pelo que o conteúdo dos ficheiros não é copiado.
- **Alt-V:** escolhe uma coleção guardada numa lista com deslocamento que mostra quantas entradas
  tem cada uma e uma estimativa do seu tamanho. Prima F7 ou N para criar uma. As coleções são
  mantidas quando o Borg reinicia.
- **Esc:** oculta os resultados e mantém o cursor, a posição de deslocamento, as marcas, o filtro
  e o agrupamento.
- **Alt-Shift-R:** volta a mostrar os resultados ocultos. Uma análise continua a decorrer enquanto
  os seus resultados estão ocultos.
- **Alt-D:** abre Tamanhos das pastas para encontrar pastas pelo tamanho total ou pelo número de
  ficheiros. Os ficheiros ocultos são incluídos por omissão. Alt-D abre a caixa de diálogo de Ctrl-F
  com *Totais de pastas* selecionado; altere *Procurar* para alternar entre os dois modos. Ctrl-C
  interrompe uma análise.
- **Alt-S:** calcula os tamanhos da pasta selecionada. **Alt-Shift-D:** abre uma árvore de
  tamanhos da pasta atual.
- **Alt-W:** maximiza ou restaura o painel de resultados ativo. Os resultados abrem num painel
  normal por omissão. Os controlos `[+]` (maximizar), `[=]` (restaurar) e `[-]` (recolher) na
  barra de título podem ser clicados. Recolher oculta a vista; Alt-Shift-R repõe-na com o mesmo
  esquema e a mesma seleção.
- **Ctrl-S:** filtra os resultados por nome ou caminho de origem. Um filtro vazio mostra tudo.

### Pesquisar dentro dos ficheiros

Escreva texto em **Com o texto** na caixa de diálogo de Ctrl-F para listar os ficheiros abaixo da
pasta que o contêm. O padrão de nome continua a aplicar-se, por isso `*.rs` com `fn main` em **Com
o texto** encontra os ficheiros Rust que o têm. As opções ao lado do campo funcionam como na barra
de pesquisa: Alt-C distingue maiúsculas de minúsculas (caso contrário, uma pesquisa em minúsculas
encontra qualquer caixa), Alt-W procura palavras inteiras e Alt-R lê o texto como expressão
regular. Também pode clicar nas opções. Ctrl-P e Ctrl-N no campo trazem de volta pesquisas recentes
da barra de pesquisa e de pesquisas anteriores. Uma pesquisa de texto lista apenas ficheiros, por
isso não aceita totais de pastas nem um mínimo de ficheiros, e **Tamanhos das pastas** não tem o
campo **Com o texto**.

Cada resultado mostra a primeira linha com correspondência, com o número da linha, na coluna
**Correspondência**, e a linha de estado junta o número da linha ao caminho do ficheiro. F3 abre o
ficheiro no visualizador nessa correspondência, com a barra de pesquisa aberta na pesquisa, para que
Enter avance para a seguinte. Os ficheiros Markdown e JSON abrem aí como código-fonte, porque a
pesquisa leu o ficheiro tal como está, e `s` muda para a vista formatada. F4 e Alt-E abrem o
ficheiro no editor do Borg nessa correspondência, com a correspondência selecionada e a barra de
pesquisa aberta na pesquisa. O Borg lê os ficheiros em segundo plano, e a linha de contexto conta os
ficheiros lidos. Os ficheiros binários (um byte NUL nos primeiros 8 KiB) e os ficheiros com mais de
64 MiB ficam por ler e contam como binários ou demasiado grandes. Ctrl-C interrompe a pesquisa, que
para nos 20 000 ficheiros como qualquer pesquisa.

### Trabalhar com os resultados

Clique no cabeçalho de uma coluna para ordenar por ela e clique de novo para inverter a ordem. Os
menus Esquerda e Direita também permitem ordenar, e o menu Pesquisar acrescenta a ordenação por
pasta de origem e o agrupamento por pasta ou tipo de ficheiro. Enter no cabeçalho de um grupo
recolhe-o; Insert marca os ficheiros correspondentes desse grupo. As marcas mantêm-se quando
ordena ou filtra, e o rodapé conta também os ficheiros marcados que o filtro oculta.

F3 e Ctrl-Q abrem o ficheiro original. F5 copia para uma pasta real; se o outro painel for uma
coleção, F5 acrescenta-lhe referências. F6 precisa de uma pasta real como destino. Numa coleção,
F8 remove referências e não mexe nos ficheiros; nos resultados de pesquisa, F8 pede confirmação
para eliminar os ficheiros originais. Shift-F8 também elimina os ficheiros originais a partir de
uma coleção, depois de confirmar. Quando estão marcados uma pasta e ficheiros dentro dela, a
tarefa trata a pasta uma única vez. **Revelar original** abre a pasta do ficheiro no outro painel.
Left ou Esc ocultam os resultados, e Alt-Shift-R repõe-nos onde os deixou. Tab alterna entre os
painéis mesmo quando a shell tem um comando por terminar. A shell fica sempre numa pasta real. F10
pede sempre confirmação antes de sair.

Por agora, as ações de Git e de arquivos só funcionam em painéis de pastas reais. Para as usar
num ficheiro de uma coleção, abra primeiro a respetiva pasta com **Revelar original**.

### Limites da pesquisa

A pesquisa lê nomes de ficheiros e metadados num processo em segundo plano que pode cancelar, e
para às 20 000 correspondências. A linha de contexto indica se a pesquisa terminou, os erros e as
entradas ignoradas, e se atingiu esse limite. As pastas ocultas só são pesquisadas com **Mostrar
ficheiros ocultos** ativo. A pesquisa não sai do sistema de ficheiros onde começou e não segue
ligações simbólicas para pastas. A janela de pesquisa pode limitar os resultados a ficheiros ou a
pastas e definir um tamanho mínimo.

### Totais de pastas e árvore de tamanhos

Escolha **Totais de pastas** para medir todos os ficheiros abaixo de cada pasta antes de filtrar
por nome, tamanho ou número mínimo de ficheiros. O Borg guarda as 20 000 maiores pastas
correspondentes e ordena-as por tamanho; clique em Ficheiros para as ordenar pelo número de
ficheiros. Use um padrão como `node_modules` ou
`*cache*`, ou `*` para ver todas as pastas abaixo da pasta inicial.

Os totais somam o conteúdo dos ficheiros. Eliminar uma pasta pode libertar uma quantidade
diferente de espaço em disco. As subpastas também contam para os totais das pastas que as contêm. As ligações simbólicas nunca são
seguidas, as análises não saem do sistema de ficheiros onde começaram, e uma pasta que se contém
a si própria (através de uma montagem bind ou de uma pasta com ligação física) só é contada uma
vez. Quando um total fica incompleto por causa de um destes limites, ou porque uma subpasta não
pôde ser lida ou foi omitida, aparece com ≥. As análises de pastas leem apenas metadados, nunca o
conteúdo dos ficheiros. Os resultados aparecem quando a medição termina, e pode cancelar a
análise a qualquer momento.

**Árvore de tamanhos das pastas** mostra no painel atual o total da pasta inicial e das suas
subpastas. Right expande um ramo, ou entra nele se já estiver expandido; Left recolhe um ramo ou
seleciona o ramo pai. Enter abre a própria pasta. Clique na seta de um ramo para o recolher ou
expandir, ou no cabeçalho de uma coluna para ordenar as pastas do mesmo nível por nome, tamanho ou
número de ficheiros. Esc oculta a árvore, e Alt-Shift-R repõe os ramos, as marcas e o cursor. As
operações com ficheiros atuam sobre os caminhos reais e, se marcar uma pasta e uma subpasta dela,
uma tarefa de cópia ou eliminação recebe apenas a pasta.

### O que as pesquisas omitem

Com **Omitir resultados de compilação, caches e ficheiros ignorados** assinalado, as pesquisas
por nome ignoram pastas geradas: `node_modules`, `.git`, caches e ambientes virtuais de Python,
pastas como `target` e `dist` que estejam ao lado de um ficheiro de compilação (`Cargo.toml`,
`package.json`), e qualquer pasta marcada com um ficheiro `CACHEDIR.TAG`. Ignoram também os
ficheiros excluídos por `.gitignore`,
`.ignore`, `.rgignore` ou `.borgignore`, e os globs definidos em `[search] exclude`.

Para alterar o que fica de fora, prima F4 na caixa de diálogo de pesquisa ou clique em **F4
Editar**. Pode ativar ou desativar as pastas geradas e os ficheiros de exclusão, e adicionar
(Ins), editar (Enter) ou remover (Del) padrões. As alterações são guardadas no ficheiro de
configuração. Uma pasta omitida continua a aparecer se o próprio nome corresponder, e a linha de
contexto indica quantas pastas foram omitidas. Acrescente `excluded:yes` a uma pesquisa para
procurar em tudo nessa vez, ou `hidden:yes` para incluir ficheiros ocultos. As análises de
tamanho leem sempre tudo.

No macOS, as pesquisas e as análises de tamanho que começam acima de `~/Library/Containers` e
`~/Library/Group Containers` ignoram estas pastas: ler aí faz com que o macOS suspenda a leitura
até alguém responder se autoriza o acesso aos dados de outra aplicação. Uma pesquisa iniciada
dentro de uma destas pastas lê-a normalmente.

O histórico de pesquisa fica neste computador e só o utilizador o pode ler. Defina
`history = false` em `[search]` para deixar de o guardar.

### Pesquisa instantânea com índice

**Pesquisar → Indexar esta pasta para pesquisa instantânea** mantém um índice de todos os nomes
abaixo da pasta ativa, para que Ctrl-F responda de imediato nessa pasta ou em qualquer pasta
abaixo dela. A linha de contexto indica quando o índice foi criado. O Borg reconstrói os índices
em segundo plano depois de arrancar e sempre que um índice tem mais de 15 minutos, e o índice
antigo continua a responder até o novo estar pronto. Uma pesquisa que não omite nada, ou que usa
exclusões diferentes, lê as pastas diretamente, tal como uma pesquisa de texto, porque o índice só
guarda nomes. Escolha de novo a mesma opção do menu para deixar
de indexar e eliminar o índice. Os índices ficam na pasta de cache, junto às análises de tamanho,
e `[index]` na configuração enumera as pastas indexadas.

### Contar tamanhos em segundo plano

Para medir tamanhos sem fechar os painéis de ficheiros normais, prima **Alt-I** ou escolha
**Pesquisar → Contar tamanhos em segundo plano**. O Borg analisa todas as pastas abaixo da atual,
incluindo ficheiros ocultos, e preenche o total de cada pasta à medida que termina. Pode continuar
a navegar ou usar a consola enquanto a análise decorre. As pastas que ainda estão a ser medidas
mostram um total crescente assinalado com `≥`, e os totais mantêm-se quando sai de uma pasta e
volta a ela. O rodapé do painel mostra o progresso; prima **Alt-I** de novo, ou clique no rodapé,
para parar a análise e manter os totais medidos até esse momento. Iniciar uma tarefa de cópia,
movimentação ou eliminação para uma análise de tamanhos em curso. As análises concluídas partilham
a cache com a árvore de tamanhos e com Totais de pastas.

### Análises de tamanho guardadas

As análises concluídas ficam guardadas neste computador. Totais de pastas e a árvore de tamanhos
reutilizam uma análise guardada da mesma pasta com a mesma definição de ficheiros ocultos. Quando
volta a abrir uma pasta que já foi medida, os tamanhos guardados aparecem de imediato, assinalados
com `~`, com a data da medição por cima da árvore. **Ctrl-R** volta a analisar em qualquer das
vistas de tamanho, e **Ctrl-C** cancela. A árvore anterior continua visível durante a atualização
e mantém-se se a nova análise for cancelada ou falhar. As listagens normais de pastas também
mostram os totais guardados, quando existem. Alterações feitas fora do Borg podem desatualizá-los,
e a navegação nunca inicia uma análise recursiva por si só. As tarefas de cópia, movimentação e
eliminação do Borg descartam as análises guardadas que abrangem os caminhos alterados.

A cache SQLite local guarda todas as pastas medidas para até 16 pastas iniciais. As árvores
carregam as subpastas de um ramo quando o expande. Um ramo mostra até 20 000 pastas, e Totais de
pastas guarda as 20 000 maiores correspondências; estes limites de apresentação não interrompem a
análise nem alteram os totais. Uma linha "Ficheiros nesta pasta" contabiliza os bytes guardados
diretamente numa pasta expandida. Os erros e as subpastas ignoradas marcam os totais como
incompletos. A cache fica em
`~/.cache/borg-commander`, ou em `XDG_CACHE_HOME` se estiver definido; `BORG_CACHE_DIR` substitui
essa localização. A cache contém apenas medições que podem ser recriadas e é independente das
coleções. Um ficheiro com ligações físicas conta uma vez por cada caminho que tem. Todos os
tamanhos medem o conteúdo dos ficheiros, pelo que o espaço que um ficheiro ocupa em disco pode
ser maior ou menor.

## Teclas

A barra de atalhos de duas linhas fica por baixo da linha de comando. A primeira linha mostra as
teclas de função; a segunda mostra a pesquisa, as coleções e outros atalhos que dependem do que
está a fazer. Pode clicar nas duas linhas. As notificações substituem a segunda linha durante uns
instantes.

| Tecla | Ação |
| --- | --- |
| Up, Down, PgUp, PgDn, Home, End | Mover |
| Enter, Right | Abrir uma pasta / executar ou ver um ficheiro |
| Left | Ir para a pasta pai |
| Tab | Alternar entre painéis, ou entrar na pré-visualização e voltar à lista de ficheiros |
| Ctrl-O | Mostrar a shell / voltar aos painéis |
| Alt-I | Contar tamanhos de pastas em segundo plano / parar a contagem |
| Ctrl-S | Localização rápida na pasta |
| Ctrl-Q | Pré-visualização: mostra o ficheiro sob o cursor (texto, imagem ou pasta) no outro painel |
| Ctrl-L | Informações: mostra tamanho, permissões, proprietário, datas e espaço livre no outro painel |
| Insert, Ctrl-T, Shift-Up/Down | Marcar ficheiros |
| `+` `-` `*` | Selecionar, desselecionar ou inverter por padrão (`*.rs`, `build/`) |
| Alt-Enter | Colocar os nomes marcados (ou o atual) na linha de comando |
| Ctrl-Insert / Alt-Shift-Insert | Copiar os nomes marcados / os respetivos caminhos completos |
| Shift-Enter, Ctrl-J | Inserir uma quebra de linha no cursor da linha de comando; Enter executa o comando, Ctrl-C limpa-o |
| Alt-A / Alt-Shift-A | Colocar na linha de comando o caminho da pasta deste painel / do outro painel |
| Shift-PgUp / Shift-PgDn | Percorrer a saída da consola para trás e para a frente |
| Shift-Home / Shift-End | Ir para a saída mais antiga da consola / voltar ao prompt ativo |
| Ctrl-P / Ctrl-N, Ctrl-R | Histórico da shell: comando anterior / seguinte, pesquisa (tratados pela sua shell) |
| Alt-. | Mostrar ficheiros ocultos |
| Alt-R | Recarregar o painel ativo: a pasta, os resultados de pesquisa ou a coleção |
| Alt-H | Hosts remotos: máquinas emparelhadas e as respetivas sessões |
| Alt-Shift-Left/Right | Redimensionar os painéis |
| Ctrl-U | Trocar os painéis |
| Alt-J | Tarefas: pausar ou cancelar cópias, movimentações e eliminações em curso |
| Alt-M | Monitor de atividade: processos e memória |
| Alt-C | Docker: projetos Compose e contentores |
| Alt-T | Time Machine: o que as cópias de segurança deixam de fora, no macOS |
| Alt-E | Editar o ficheiro selecionado neste painel, em vez dos seus ficheiros |
| F1 | Sobre |
| F2 | Menu Ficheiro |
| F3 | Ver (texto ou imagem) |
| F4 / Shift-F4 | Editar no editor do Borg / em `$VISUAL` ou `$EDITOR` |
| F5 / F6 | Copiar / mover para o outro painel |
| F7 | Criar uma pasta |
| F8 | Eliminar |
| F9 | Barra de menus |
| F10 | Sair |

Tab, Enter, Home, End, Left e Right controlam os painéis enquanto a linha de comando está vazia.
Assim que começa a escrever um comando, estas teclas passam para a shell. Ctrl-P, Ctrl-N e Ctrl-R
usam o histórico de comandos da sua shell. Shift-Enter (ou Ctrl-J) insere uma quebra de linha sem
executar o comando. Os comandos com várias linhas, escritos ou recuperados do histórico, tornam a
área do prompt mais alta e os painéis mais baixos, para que o cursor fique sempre visível. Todas
as teclas habituais de edição e de conclusão da sua shell continuam a funcionar.

O que copia chega à área de transferência do computador através do terminal, mesmo por SSH e
num espelho que controle. O Borg guarda também as últimas 20 cópias: Ctrl-V ou Shift-Insert cola
a mais recente nos campos do próprio Borg, como as caixas de diálogo e a pesquisa de memória,
mesmo quando o terminal não tem acesso à área de transferência. Na linha de comando, Ctrl-V
pertence à shell, por isso cole aí com o atalho de colar do terminal. O menu Ficheiro oferece as
duas ações de cópia para teclados sem tecla Insert. As cópias demasiado grandes para o terminal
transmitir ficam no Borg.

Se o terminal reservar as teclas F para si, prima **Esc e depois um algarismo** no espaço de 1,5
segundos: Esc 1 a Esc 9 equivalem a F1 a F9, e Esc 0 a F10. Por exemplo, Esc 9 abre os menus e Esc 5
copia. Enquanto o Borg espera pelo algarismo, a linha das teclas de função mostra qual premir. Se
houver algo aberto (uma notificação, resultados, a pré-visualização ou Informações), o primeiro Esc
fecha-o e um segundo Esc cancela. Alt-1 a Alt-0 também funcionam quando o terminal deixa passar a
tecla Alt. No macOS, pode ser necessário ativar "Use Option as Meta key" (usar Option como tecla
Meta) no terminal.

## Rato e consola

Clique numa linha para lhe mover o cursor, faça duplo clique para a abrir e clique com o botão
direito para a marcar. Clique na barra de menus ou na barra de atalhos para as usar. A roda do
rato percorre o painel.

As caixas de diálogo também funcionam com o rato. Clique numa linha de uma lista para a escolher;
nos seletores de tema e de idioma, em Hosts remotos e em Acesso remoto, um segundo clique
confirma. A roda percorre listas como as de temas, definições e tarefas. As teclas indicadas no
contorno de uma caixa de diálogo podem ser clicadas: clique em **Esc Fechar** para fechar a caixa,
ou num botão para responder a uma pergunta. Clicar fora de uma lista, de um seletor ou de Sobre
fecha-os, enquanto os formulários e as perguntas ficam abertos até responder.

Ctrl-O alterna entre os painéis e a consola. Antes de escrever ou de abrir a consola, a shell
muda para a pasta do painel ativo. No zsh isto acontece de forma silenciosa e o prompt é
redesenhado; no bash e no fish, vê na consola um breve comando `cd`. Se tiver aberto a consola
por iniciativa própria, ela continua aberta depois dos comandos e de Ctrl-C. "Regressar aos
painéis após o comando" aplica-se apenas aos comandos executados a partir dos painéis. No prompt
da shell, Option-Left e Option-Right (Alt no Linux) avançam palavra a palavra.

Na consola, a roda do rato percorre a saída para trás sem mexer no comando que está a escrever.
Shift-PgUp e Shift-PgDn percorrem uma página de cada vez, Shift-Home salta para a saída mais antiga e
Shift-End volta ao prompt ativo, tal como começar a escrever. Para copiar a saída visível,
selecione-a da forma que o terminal permitir, normalmente arrastando com Shift, e use o atalho de
copiar habitual.

## Plugin Git

O suporte para Git é um plugin incluído e está ativo por omissão. A caixa de verificação
encontra-se em F9 → Opções → Plugins; desativá-la oculta o menu Git e as colunas de estado e
interrompe as verificações de estado em segundo plano. O Git tem de estar instalado e no PATH.

Dentro de um repositório, os painéis mostram o ramo atual e um estado de dois caracteres junto a
cada ficheiro. O carácter da esquerda refere-se ao índice (alterações preparadas) e o da direita à
árvore de trabalho. `M` significa modificado, `A` adicionado, `D` eliminado, `R` mudado de nome,
`??` não seguido e `U` um conflito. As pastas resumem as alterações que contêm. Os ficheiros sem
alterações e os ignorados não têm marcador. Fora de um repositório, os nomes dos ficheiros ocupam
o espaço extra.

O menu Git mostra o estado do repositório, as diferenças não preparadas e preparadas e os commits
recentes, e pode preparar ou retirar da preparação os ficheiros marcados (ou o ficheiro atual, se
nada estiver marcado). As diferenças e o histórico abrem no visualizador normal. Retirar da
preparação não altera os ficheiros de trabalho. O estado é atualizado quando muda de pasta,
executa comandos ou usa ações do Git, e de cinco em cinco segundos enquanto os painéis estão
inativos. **Atualizar estado**, no menu Git, volta a verificar de imediato.

## Plugin Docker

**Alt-C** ou **Comando → Docker** lista os seus contentores no painel ativo, agrupados por
projeto Compose, com os contentores que não pertencem a nenhum projeto no fim. O outro painel
mostra o contentor selecionado: estado, portas, redes, montagens, comando e política de
reinício, além da utilização de CPU, memória, rede e disco em tempo real, com um gráfico do
último minuto. Num projeto, mostra a pasta do projeto, os seus contentores e a utilização
conjunta. Esc fecha a vista.

| Tecla | Ação |
| --- | --- |
| ↑ ↓, PgUp, PgDn, Home, End | Mover |
| ← → / Enter | Recolher ou expandir um projeto; Enter num contentor passa para os detalhes |
| S / X / R | Iniciar / parar / reiniciar o contentor, ou os contentores do projeto |
| P | Pausar ou retomar |
| F | Explorar os ficheiros do contentor |
| E, Alt-2 | Abrir uma shell no contentor, ou voltar à shell já aberta |
| Alt-1 | Voltar a mostrar a visão geral do contentor, também a partir da shell |
| `/` | Filtrar por nome, serviço, imagem ou projeto |
| Tab | Alternar entre a lista e os detalhes |
| Alt-W | Maximizar o painel com o foco |

Parar e reiniciar pedem confirmação primeiro. Num projeto, uma ação só afeta os contentores que
alteraria, pelo que Começar só inicia os contentores parados.

### Ficheiros do contentor

**F** num contentor em execução fecha a vista Docker e mostra o sistema de ficheiros do contentor
nesse painel, a começar no diretório de trabalho. O título do painel mostra o nome do contentor. O
outro painel continua na pasta do host, pelo que **F5** copia em ambos os sentidos, enquanto
**F3** mostra, **F7** cria uma pasta e **F8** elimina dentro do contentor. A pesquisa, os
tamanhos, as pré-visualizações e a edição só funcionam no host. **Alt-C** volta à vista
Docker, e `..` na raiz do contentor devolve o painel à pasta do host que mostrava antes.
Explorar exige `sh` no contentor, pelo que não é possível listar imagens mínimas sem shell.

Enquanto um painel mostra os ficheiros de um contentor, a linha de comando acompanha-o: passa a
ser a shell desse contentor, iniciada na pasta do painel, com o nome do contentor no início do
prompt. Os comandos são executados dentro do contentor, o painel é atualizado quando terminam e
`cd` aí move o painel. Ctrl-O mostra a consola desse contentor, assinalada como **▣ nome
consola**. Num painel do host, a linha de comando e Ctrl-O voltam a pertencer ao host, e
o que lá tinha escrito continua lá. Trata-se da mesma shell do separador Shell do contentor na
vista Docker.

Para manter a linha de comando no host, desative **Seguir** em
Opções → Plugins. Nesse caso, enquanto um painel mostra os ficheiros de um
contentor, a linha de comando fica esbatida e assinalada com **⌂ host**, e Enter não faz nada até
um painel do host estar ativo, para que um comando nunca seja executado na máquina errada.

### Shells de contentores

**E** abre uma shell no contentor (bash, se a imagem o tiver; caso contrário, `sh`) no separador
Shell do painel de detalhes, e envia-lhe as teclas, incluindo as teclas de função. **Alt-1**
mostra a visão geral, **Alt-C** volta à lista e a roda do rato percorre para trás a saída da
shell. Uma shell continua em execução enquanto vê outros contentores, fecha a vista Docker ou usa
a consola do host; o respetivo contentor fica assinalado com `›_` na lista e é contado no
atalho Alt-C. A
shell fecha quando sai dela, quando o contentor para ou quando o Borg termina (o Borg pede
confirmação primeiro se houver shells abertas).

O plugin usa a CLI `docker` e o contexto atual, pelo que funciona igualmente com Docker Desktop,
OrbStack, Colima e motores remotos. Pode desativá-lo em Opções → Plugins.

## Exclusões do Time Machine

No macOS, **Alt-T** ou **Comando → Time Machine** mostra o que o Time Machine deixa fora das
cópias de segurança e permite alterá-lo. Os painéis assinalam com ⊘ os itens que estão excluídos
por si próprios, e o rodapé conta-os ou indica que a pasta inteira não tem cópia de segurança.
**Ctrl-L** (Informações) mostra como o Time Machine trata o item sob o cursor.

A janela tem três separadores; prima **1**, **2**, **3** ou **Tab**, ou clique num separador,
para mudar. **Seleção** lista os itens marcados, ou o item sob o cursor. **Caches aqui** procura
na pasta do painel pastas de cache e de compilação como `node_modules`, `target`, `.venv` e
pastas com um `CACHEDIR.TAG`, das maiores para as menores, e soma quanto delas ainda entra nas
cópias de segurança. **Todas as exclusões** lista todas as exclusões que o Borg consegue
encontrar, por tipo: itens marcados que o Spotlight conhece e os que estão na pasta do painel,
caminhos fixos, volumes excluídos e os locais que o macOS deixa sempre de fora.

**Space**, **Insert** ou um clique com o botão direito marcam linhas, e `*` inverte as marcas.
**E** exclui as linhas marcadas, ou a que está sob o cursor, marcando o próprio item: não precisa
de palavra-passe e a exclusão acompanha o item, mas perde-se se uma ferramenta eliminar e voltar a
criar a pasta. **P** exclui antes o caminho, que fica excluído seja o que for que lá esteja, e
pede a palavra-passe na caixa de diálogo do sistema. **I**, **Delete** ou **F8** voltam a incluir
itens. Para voltar a incluir nas cópias de segurança um item que está dentro de uma pasta
excluída, inclua a pasta. O que o macOS deixa de fora por si não pode ser alterado.

Em Todas as exclusões, **A** exclui um caminho que escreva, mesmo que ainda não exista nada nesse
local. **R** procura de novo, **Enter** ou um segundo clique numa linha mostra o item no painel
ativo, e **Esc** fecha a janela. Excluir ou incluir um volume também pede a palavra-passe. Para
indicar como trata algumas pastas, o Time Machine pode precisar de que o terminal tenha Acesso
total ao disco em Definições do Sistema › Privacidade e segurança; até lá, essas pastas aparecem
como Desconhecido. Nos outros sistemas, esta opção do menu fica oculta.

## Claude e outros agentes (MCP)

`borg mcp` permite que um agente de IA como o Claude trabalhe no borg que tem aberto, seja no seu
próprio computador, seja num servidor a que acede por SSH. Não precisa de instalar mais nada além
do borg: o agente inicia `borg mcp` por si (através de SSH, no caso de um servidor) e este liga-se
à sessão do borg que está a correr nessa máquina. O servidor não precisa de portas abertas nem de
acesso à internet.

### Ligar o Claude

Prima F1 e depois C (ou escolha F9 → Remoto → Partilhar este Borg como host…). O Borg mostra
dois comandos para colar num terminal do seu computador: um inicia o Claude Code já ligado a este
borg; o outro acrescenta a ligação ao Claude Code para todas as sessões. Tab escolhe um deles e
Enter copia-o para a área de transferência do computador através do terminal. Isto funciona por
SSH no iTerm2, Ghostty, kitty, WezTerm e Alacritty (no tmux, acrescente `set -g set-clipboard on`).
Quando o terminal não o aceita, como acontece em muitos terminais de browser, F3 mostra o comando
isolado em texto simples, para o poder selecionar com o rato; qualquer tecla volta ao borg.

Por SSH, o borg preenche o endereço a que se ligou. Se o seu computador chegar ao servidor por
outro nome, como um alias de SSH ou através de um jump host, escreva antes esse nome. O SSH tem de
iniciar sessão sem pedir palavra-passe, com uma chave ou um agente SSH. `borg mcp setup prod`
mostra na linha de comando a mesma configuração para o Claude Code e o Claude Desktop.

### O que o agente pode fazer

Depois de ligado, o agente vê o mesmo que o utilizador: o próprio ecrã em texto, quer esteja nos
painéis quer na consola, os dois painéis, os ficheiros realçados e marcados, onde corre a linha de
comando e o que está escrito nela. Refere-se a sítios como localizações: um caminho absoluto ou
`~/…` na máquina onde o borg corre, `docker://web/etc/nginx` dentro de um contentor, ou um caminho
relativo ao painel ativo. Sem pedir autorização, pode ler pastas e ficheiros (por intervalo de
linhas, ou apenas as linhas que contêm determinado texto), ver os processos mais ativos e os
contentores, e mostrar um ficheiro ou uma pasta no seu outro painel.

Tudo o que altera a máquina fica à sua espera numa caixa de diálogo que mostra a alteração
completa: um comando e o sítio onde vai ser executado, um ficheiro novo ou uma edição sob a forma
de diff. Percorra as alterações longas com as setas, PgUp e PgDn ou a roda do rato, e prima F3
para ver a caixa de diálogo em ecrã inteiro. A opção Não mantém-se selecionada até escolher Sim, e uma edição é
recusada se o ficheiro tiver mudado enquanto decidia.
Para um computador emparelhado, `approval_mode` pede-lhe confirmação uma vez antes de ativar
Aprovar tudo nessa sessão. Enquanto estiver ativo, fica visível um distintivo; clique nele ou prima
Ctrl+Alt+A para o desativar de imediato. Desativa-se também quando essa ligação emparelhada fecha.
As aprovações de emparelhamento são independentes.

Os comandos são escritos numa shell que pode acompanhar e ficam fora do respetivo histórico. Os
comandos para esta máquina vão para a consola do próprio borg, que é mostrada enquanto correm para
poder responder a um pedido como o do sudo; os comandos para um contentor vão para a shell desse
contentor na vista Docker. A shell mantém a pasta e as variáveis entre comandos.

O agente também pode escrever texto para o utilizador rever: um comando no prompt, ou um script
no vim ou no nano abertos na consola. Nada é executado: Enter nunca é premido, e o texto com várias
linhas entra como colagem, que as shells e os editores inserem sem executar.

O agente pode ainda pesquisar a memória de um processo da mesma forma que a pesquisa de memória,
à procura de cadeias de caracteres, indicadores, texto ou bytes. A memória pode conter
palavras-passe, chaves e mensagens privadas, e tudo o que a pesquisa encontrar vai para o agente;
por isso, a primeira pesquisa em cada processo aguarda a sua aprovação numa caixa de diálogo que
indica o processo, o programa e a pesquisa. Depois de aprovar, o agente pode voltar a pesquisar
esse processo até o borg terminar. Pode pedir para ler como administrador, o que abre o pedido de
palavra-passe do próprio sistema.

Enquanto houver um agente ligado, **◈ Remoto** aparece no início da barra de atalhos. Clique nele
para ver um registo do que o agente fez: o que viu, leu, mostrou e escreveu, e todos os comandos e
alterações, aprovados ou recusados. O Borg guarda o registo apenas em memória, até terminar; para
não guardar registo nenhum, desative-o em Opções → Plugins.

### Borg cloud

O borg cloud permite que o Claude chegue a um borg para o qual não tem caminho SSH: um servidor
que abre num terminal de browser, um contentor ou uma máquina atrás de NAT. Ambos os lados se
ligam a um relay, e o relay junta-os.

1. No borg do servidor, prima F1 e depois C. A primeira opção é `borg pair` com uma ligação; Enter
   copia-a. A ligação funciona uma única vez, durante dez minutos. Por baixo fica o relay: para usar
   outro relay, cole aí o respetivo endereço e prima Enter para o guardar e obter uma ligação
   através dele.
2. Execute o comando num terminal do seu computador, onde o borg também esteja instalado. É
   apresentado um código de emparelhamento: seis emojis, cada um com o seu nome, como
   `🐱 Cat · 🦁 Lion · 🐎 Horse`.
3. O borg do servidor pergunta se quer emparelhar, mostrando o nome do seu computador e o código.
   Escolha Sim se os códigos coincidirem. Em seguida, `borg pair` propõe acrescentar o servidor ao
   Claude Code, como `claude mcp add borg-NAME -- borg mcp remote NAME`.

O código deriva do handshake cifrado que os dois borgs acabaram de concluir, pelo que só é igual
nos dois ecrãs se for o seu computador a comunicar com o servidor. Alguém que tivesse obtido a
ligação e a usasse primeiro mostraria ao servidor um código diferente, e não consegue escolher um
que coincida com o seu. Se o terminal desenhar os emojis mais estreitos do que o borg espera, o
borg mostra apenas os nomes; nesse caso, compare os nomes.

A partir daí, o Claude pode chegar a esse borg sempre que estiver a correr nessa máquina.
`borg pair` sozinho lista os servidores emparelhados, e `borg pair --forget NAME` esquece um
deles; no servidor, F8 na caixa de diálogo Acesso remoto esquece todos os computadores
emparelhados. Através do borg cloud, o Claude obtém também
`download` e `upload`, que copiam ficheiros entre o servidor e o seu computador;
`borg get NAME:PATH [TO]` e `borg put FROM NAME:PATH` fazem o mesmo a partir do terminal. Os
envios aguardam a sua aprovação no borg, e cada aprovação indica o computador que fez o pedido.

A ligação é cifrada de ponta a ponta. Cada borg guarda uma chave privada, que só o utilizador pode
ler, na sua pasta de estado. O emparelhamento troca as chaves públicas, e cada ligação abre com um
handshake Noise IK associado a elas, pelo que o relay só reencaminha texto cifrado e não consegue
responder em nome de nenhum dos borgs. Os pedidos vindos de outro computador podem ler sem pedir
autorização, tal como os locais; ative "perguntar antes de outro computador ler" em Opções →
Plugins para que também essas leituras aguardem aprovação. O Borg não mantém nenhuma ligação ao
relay a menos que esteja emparelhado ou a mostrar uma ligação de emparelhamento, e não mantém
nenhuma quando o borg cloud está desativado em Opções → Plugins.

O Borg guarda o relay na configuração como `relay = "wss://relay.example.com"`. Um computador
chega ao servidor através do relay pelo qual emparelhou, por isso, depois de o servidor mudar de
relay, volte a emparelhar o computador. Todos os borgs de uma máquina partilham uma chave, pelo
que só o primeiro a ser aberto se liga ao relay; os outros indicam qual deles tem o borg cloud.

### Sessões com nome e serviço em segundo plano

Com espaços de trabalho com nome, `borg sessions list` mostra os respetivos IDs e nomes,
`borg sessions create NAME` acrescenta um e `borg --session NAME` abre-o. Através de MCP, o agente
pode chamar `list_sessions`, `list_remote_sessions` e `use_session` para escolher a sessão com que
essa ligação trabalha. Uma única chamada de ferramenta pode também passar um ID em `session` para
visar outra sessão apenas dessa vez. A sessão escolhida aplica-se apenas a essa ligação MCP.

`borg service install`, `enable` e `start` configuram o serviço opcional em segundo plano. Este
mantém a ligação de emparelhamento da máquina e mantém as sessões com nome em execução depois de
os respetivos terminais fecharem. `borg service attach NAME` volta a ligar-se a uma delas, e
Ctrl-] desliga-se de novo sem terminar a shell. Use `borg service stop`, `disable` ou `uninstall`
para parar ou remover o serviço.
Num contentor sem systemd, configure o supervisor de processos do contentor para executar
`borg service run`; os comandos `install` e `enable` precisam de um host Linux com systemd.

### Painéis remotos

Depois do emparelhamento, `borg --remote MACHINE:SESSION` abre os ficheiros dessa sessão no painel
esquerdo. Enter ou F3 pré-visualiza um ficheiro remoto transferindo uma cópia local temporária.
**F5** copia ficheiros e pastas marcados entre painéis locais e remotos, ou entre dois hosts
emparelhados, e a caixa de diálogo de destino indica que cliente ou host está de cada lado.
As cópias correm como tarefas em segundo plano, com progresso em bytes, pausa e cancelamento, e a
opção de substituir, ignorar ou manter ambos. As pastas são copiadas com as subpastas vazias e as
ligações simbólicas, e os ficheiros normais mantêm a permissão de execução e a data de
modificação. Um ficheiro só aparece no destino depois de todo o seu conteúdo ter chegado. F6
(mover) e F8 (eliminar) não estão disponíveis em painéis remotos.

O painel direito fica no seu computador. Com **Opções → Plugins → Seguir** ativo, a linha de
comando e **Ctrl-O** usam uma shell interativa no host remoto. O terminal é desenhado
localmente, enquanto as teclas, as colagens, Ctrl-C e os programas de terminal correm no
host remoto. Cada sessão remota mantém a sua shell e a linha de comando por terminar enquanto
muda de painel, e selecionar um painel do cliente traz de volta a shell local.

Os programas de ecrã inteiro, e os que leem diretamente o teclado, como `top`, recebem todas as
teclas enquanto correm, incluindo Ctrl-O, e redimensionar o terminal redimensiona o PTY remoto.
Saia desses programas com as respetivas teclas (`q` no `top`). Se o tiver iniciado a partir da
linha de comando do painel, o Borg volta depois aos painéis; se o tiver iniciado na consola
remota, a consola fica aberta. Os painéis e a shell do próprio Borg remoto são independentes desta
consola. Para transferir ficheiros a partir da linha de comando, use
`borg get MACHINE/SESSION:PATH [TO]` e `borg put FROM MACHINE/SESSION:PATH`.

Escrever `exit` na shell principal Bash, Zsh ou Fish do Borg pede confirmação. **Não** está
selecionado por omissão e mantém em execução a mesma shell, as variáveis, a pasta e os espelhos.
Numa consola remota,
`exit` pergunta se quer fechar a shell no host; depois de Sim, escolha de novo a sessão com
Alt-H para obter uma nova.

As pastas remotas carregam por partes, com um indicador de atividade e a contagem das entradas
recebidas até ao momento; mudar para outra pasta cancela o pedido.

### Hosts remotos

No Borg local, prima **Alt-H**, clique em **Hosts remotos** na barra de atalhos ou escolha
**F9 → Remoto → Explorar hosts e sessões…** para ver as máquinas emparelhadas e as respetivas
sessões com nome. Enter abre os ficheiros da sessão selecionada no painel a partir do qual abriu o
seletor, substituindo o que esse painel mostrava. Prima **M** para espelhar o ecrã da sessão e
**R** para atualizar a lista.

**Tab** alterna entre **Hosts** (computadores a que este Borg chega) e **Clientes**
(computadores que chegam a este Borg). Clientes lista os computadores emparelhados com este Borg,
as impressões digitais das respetivas chaves públicas, se estão ligados e quantas ligações tem
cada um. Um computador aparece uma só vez, mesmo que um espelho use várias ligações. O estado é
atualizado enquanto o seletor está aberto, incluindo numa sessão cujas ligações são mantidas pelo
serviço em segundo plano. O emparelhamento funciona num único sentido: um cliente autorizado não
passa automaticamente a ser um host cujos ficheiros este Borg possa explorar. Quando quem
detém a ligação não consegue indicar o estado da ligação, os clientes emparelhados continuam a ser
listados, com **Estado da ligação indisponível**. O mesmo Borg pode ser host e cliente ao
mesmo tempo. O menu Remoto oferece também **Espelhar host montado** e **Partilhar este Borg como
host…**.

O seletor mostra a versão do Borg de cada host. Selecionar uma sessão mostra o sistema
operativo e a arquitetura, se corre como serviço em segundo plano ou num terminal, a latência dos
pedidos e o endereço do relay. Os hosts são verificados em paralelo, pelo que um host
indisponível não atrasa os outros. Um host que não consiga indicar a versão mostra **versão
indisponível**; a versão do relay não é a do host. Mantenha o Borg atualizado nos dois
computadores: um host a que falte uma funcionalidade remota mostra uma mensagem a pedir que o
atualize.

Enquanto um espelho se liga, o seletor mostra um estado animado, e **Esc** cancela de imediato.
Cada passo da ligação tem 10 segundos, e a tentativa completa tem 30. Se a tentativa falhar, o
seletor fica aberto e indica o passo que falhou; **M** volta a tentar a sessão selecionada.

No seletor, **F2** muda o nome do host selecionado para todas as suas sessões, e **Shift-F2**
muda o nome da sessão selecionada. Estes nomes ficam guardados neste cliente e aparecem no seletor,
nos títulos dos painéis e no rodapé da consola. Deixe um nome vazio para voltar ao original. Os
detalhes continuam a mostrar os nomes originais do host e da sessão. Os nomes ficam
associados à chave do host emparelhado e ao ID da sessão; os destinos dos comandos e os nomes
no host remoto não mudam.

**F8** num host indisponível esquece o respetivo emparelhamento neste cliente, e **Shift-F8**
esquece o host selecionado mesmo que esteja online. A confirmação tem Não por omissão.
Esquecer um host fecha os painéis e as consolas deste cliente ligados a ele; as sessões e os
ficheiros do host mantêm-se, tal como a identidade deste cliente e os seus outros
emparelhamentos. Volte a emparelhar para restabelecer a ligação.

**F8** ou **Delete** numa sessão de um host remove-a, depois de uma confirmação que tem
**Não** por omissão. Isto fecha a sessão e as respetivas consolas remotas e elimina os painéis do
Borg e o histórico de terminal guardados; os ficheiros e as pastas no host mantêm-se. A
sessão Default não pode ser removida. Uma sessão autónoma que esteja a alojar a ligação não se
pode remover a si própria; feche-a primeiro no host e execute o Borg noutra sessão. Se a
remoção falhar, a sessão continua na lista para poder tentar de novo. As sessões geridas pelo
serviço são paradas antes de o seu estado guardado ser removido, e o serviço não as volta a criar.

### Espelhar uma sessão

`borg mirror MACHINE:SESSION` mostra o ecrã em direto da sessão remota no seu terminal local;
Alt+Shift+M abre o mesmo espelho a partir de um Borg local com um painel remoto. **Ctrl-G** (mantenha
Control premido e prima G) sai do espelho e volta aos painéis locais, tal como um clique na faixa
na parte inferior. A linha acima dela indica quem tem o controlo; Ctrl-] também sai.

Um `borg` normal ou `borg --session NAME` pode ser espelhado enquanto corre, sem o serviço em
segundo plano. O terminal original continua visível, e qualquer entrada de teclado ou rato nele
retoma o controlo ao espelho. O espelho segue o esquema do terminal do host e é cortado para
caber numa janela mais pequena; aumentar a janela mostra o resto. O serviço opcional em segundo
plano mantém as sessões ativas depois de o terminal original fechar e permite que o cliente que
tem o controlo redimensione o respetivo PTY.

Se outro terminal tiver o controlo da sessão remota, o espelho abre em modo só de visualização e
as suas teclas não chegam à sessão. Prima **Ctrl-T** ou clique na linha de estado para assumir o
controlo e desligar o outro terminal, ou comece com `borg mirror MACHINE:SESSION --take`; `--take`
assume sempre o controlo retirando-o a outro visualizador. Depois de ter o controlo, a primeira
tecla acorda um ecrã remoto em repouso, e as teclas e a entrada de rato seguintes chegam-lhe. Um
visualizador que fique para trás salta os fotogramas de animação antigos e passa diretamente
para o ecrã atual.

### Executar um relay

`borg-relay` é o relay através do qual os borgs se encontram. Num servidor Linux com um domínio
apontado para ele e a porta 443 livre, um só comando instala-o como serviço, com um certificado
TLS da Let's Encrypt:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install-relay.sh | sudo sh -s -- relay.example.com you@example.com
```

Execute o mesmo comando para o atualizar.

No Dokploy, no Coolify ou noutro host Docker cujo proxy trate do TLS, execute a imagem
`ghcr.io/0xcd21h/borg-relay:latest` e atribua-lhe um domínio na porta 8080. No Dokploy, crie uma
aplicação a partir dessa imagem Docker com um domínio na porta 8080 do contentor e HTTPS ativo; no
Coolify, crie um recurso a partir dessa imagem Docker com a porta 8080 exposta e um domínio.
Ambos aceitam também [packaging/relay/docker-compose.yml](../../../packaging/relay/docker-compose.yml)
como serviço Compose. A imagem executa `borg-relay --behind-proxy`: o proxy serve HTTPS e deixa
passar os WebSockets, e o relay aplica os seus limites por endereço de cliente, tal como o proxy o
indica em `X-Forwarded-For`. Corre como utilizador sem privilégios e tem uma verificação de estado
(`GET /health`).

Num host Docker sem esse proxy, a mesma imagem serve TLS diretamente:

```sh
docker run -d --restart unless-stopped -p 443:443 -v borg-relay:/data ghcr.io/0xcd21h/borg-relay --domain relay.example.com --cache /data
```

Atrás de outro proxy que trate do TLS, execute `borg-relay --behind-proxy --listen 127.0.0.1:8080`.
Execute-o desta forma apenas atrás de um proxy, porque o relay confia no endereço de cliente que o
proxy indica. O relay impõe limites por endereço para servidores e sessões, tamanho de frame,
largura de banda, bytes por sessão, tempo de inatividade e duração da sessão. Não guarda nada além
do seu certificado.

O relay regista apenas contagens: uma vez por minuto, se algo tiver mudado, escreve uma linha como

```text
stats: connections=3 servers=2 sessions=1 joined=14 carried=3.2MiB limited=0
```

Esta linha mostra as ligações que mantém, os servidores registados e as sessões abertas e, desde
que arrancou, as sessões estabelecidas, os bytes que transportaram e as ligações e sessões que um
limite recusou ou terminou. Para depuração, `--log`, ou a variável de ambiente `BORG_RELAY_LOG=1`
no caso da imagem, regista também a inscrição de cada servidor, identificado pelo início da sua chave, cada sessão
com os bytes que transportou e cada limite atingido com o endereço do cliente.

## Localizar, tarefas e visualizadores

Ctrl-S abre a localização rápida. Escreva um nome ou um glob como `*.rs`; Down e Up passam de uma
correspondência para outra, Insert marca uma correspondência, Ctrl-A marca todas e Enter ou Esc
fecham a pesquisa. A correspondência ignora maiúsculas e minúsculas, a menos que a pesquisa
contenha maiúsculas.

### Copiar, mover e eliminar

F5, F6 e F8 iniciam tarefas de cópia, movimentação e eliminação. Alt-J abre a lista de tarefas,
onde as pode pausar ou cancelar.

A caixa de diálogo Copiar tem uma caixa de verificação **Otimizar a cópia para discos rígidos**.
Prima Tab para lá chegar e Space para a assinalar ou desassinalar, ou clique nela. Fica por
assinalar em cada nova cópia. Neste modo, o Borg lê ficheiros pequenos para memória em lotes e
depois escreve cada lote, o que pode reduzir as deslocações da cabeça de leitura ao copiar dentro
do mesmo disco rígido. Um lote contém até 64 MiB de dados ou 4096 ficheiros; os ficheiros com
mais de 1 MiB são copiados normalmente. O ganho depende do disco, do sistema de ficheiros e da
cache do sistema operativo, e pode ser mais lento ao copiar entre discos diferentes.
Quando já existe um ficheiro com o mesmo nome: `o` substitui, `s` ignora, `k` mantém ambos, `a`
substitui todos,
`n` ignora todos e Esc cancela. As cópias são escritas em ficheiros temporários e mudam de nome
quando estão completas.

### Visualizador e pré-visualização

F3 abre o visualizador. Em ficheiros de texto, as setas e PgUp/PgDn percorrem o texto, `w` ativa
ou desativa a moldagem de linhas e Esc ou `q` fecham-no. Em imagens, Left/Right ou `p`/`n` mostram
a imagem anterior ou a seguinte. O Borg usa o protocolo de imagens do terminal quando existe; nos
outros terminais, a imagem aparece como blocos coloridos.

Ctrl-Q mostra uma pré-visualização no outro painel. Tab entra nela, onde as setas, PgUp/PgDn,
Home/End e a roda do rato percorrem o conteúdo; Tab ou Esc voltam à lista de ficheiros. Ctrl-L
mostra informações sobre o ficheiro. F3 abre uma pré-visualização de texto no visualizador
completo, e fechar o visualizador regressa à pré-visualização com a posição, a moldagem e o modo
de código-fonte preservados. Ambos leem os ficheiros da mesma forma limitada, pelo que abrir um
ficheiro de texto grande lê apenas mais ou menos a primeira página e continua rápido, seja qual
for o tamanho do ficheiro.

Os dois visualizadores de texto realçam a sintaxe de linguagens de programação, formatos de
marcação e ficheiros de configuração comuns, incluindo Rust, Python, JavaScript, TypeScript/TSX,
Go, C/C++, Java, C#, Ruby, PHP, Lua, SQL, HTML/CSS, JSON, YAML, TOML, scripts de shell e
Dockerfiles. O Markdown abre formatado por omissão;
prima `s` ou F4 em qualquer dos visualizadores para passar ao código-fonte. São suportados
títulos, ênfase, listas, listas de tarefas, ligações, tabelas e blocos de código delimitados. Os
ficheiros Markdown com mais de 2 MiB abrem como código-fonte para poupar memória.

O JSON abre organizado com um valor por linha, com as chaves pela ordem original e os números
exatamente como estão escritos, o que ajuda sobretudo com ficheiros escritos numa só linha. Prima
`s` ou F4 para ver o ficheiro tal como está escrito. Os ficheiros JSON Lines (`.jsonl`, `.ndjson`)
são organizados um registo de cada vez. O JSON com mais de 16 MiB, ou que não pode ser analisado,
abre tal como está escrito.

### Editor

F4 abre o ficheiro selecionado no editor do Borg, e Shift-F4 abre-o em `$VISUAL` ou `$EDITOR`. O
editor mantém os finais de linha e a indentação do ficheiro, realça o código como os
visualizadores e mostra os números de linha. Shift com as setas seleciona texto; Ctrl-C, Ctrl-X e
Ctrl-V copiam, cortam e colam através da área de transferência do Borg, e o texto colado a partir
do terminal entra exatamente como está. Ctrl-Z e Ctrl-Y anulam e refazem, e Tab e Shift-Tab
aumentam e diminuem a indentação. F2 ou Ctrl-S guardam através de um ficheiro temporário, pelo que
um ficheiro nunca fica escrito a meio. Os ficheiros que não são texto UTF-8, ou que têm mais de 32
MiB, abrem no visualizador. As linhas com mais de 20 000 caracteres, como código minificado, não
são realçadas, e só é desenhada a parte da linha que aparece no ecrã, pelo que até um ficheiro
grande escrito numa só linha se edita sem atrasos. Alt-Shift-F organiza um ficheiro JSON com um
valor por linha, usando a indentação do próprio ficheiro, num único passo que Ctrl-Z anula; o JSON
que não pode ser analisado fica como está, e a linha de estado indica onde a análise parou.

O editor faz as suas perguntas em caixas de diálogo sobre o texto: Tab passa entre campos e
botões, Enter confirma o selecionado, Esc cancela, e pode clicar em qualquer um deles. Ctrl-F ou
F7 procuram texto com a barra de pesquisa descrita abaixo. Ctrl-R ou F4 substituem, com as mesmas
opções da barra de pesquisa: Enter substitui a correspondência e passa à seguinte, **Saltar**
deixa-a como está, e Alt-A ou **Todas** substitui todas as correspondências de uma só vez, que um
único Ctrl-Z anula. Com uma expressão regular, `$1` na substituição repõe o que o primeiro grupo
encontrou. Ctrl-G vai para uma linha. Esc fecha o editor e pergunta primeiro se quer guardar ou
descartar as alterações por guardar; se o ficheiro tiver mudado no disco desde que o abriu, o Borg
pergunta se o quer substituir ou recarregar. Um clique coloca o cursor, arrastar ou clicar com
Shift seleciona, um duplo clique seleciona uma palavra e a roda percorre o texto; as teclas
indicadas na linha de estado também podem ser clicadas.

Alt-E abre o ficheiro selecionado no editor do Borg dentro do painel ativo, em vez dos seus
ficheiros, para que possa editar dois ficheiros lado a lado, ou um ficheiro ao lado da pasta onde
está. F6 passa do editor de um painel para o outro painel, e Tab volta ao editor a partir da lista
de ficheiros. O texto copiado com Ctrl-C num editor cola-se com Ctrl-V no outro. Enquanto um
editor tem o foco, a barra de atalhos mostra as teclas do editor, e as teclas do painel voltam com
a lista de ficheiros. Ctrl-O também mostra a consola a partir de um editor, e F9 abre os menus.
Esc ou F10 fecha o editor e devolve ao painel os seus ficheiros; ao sair, o Borg pergunta primeiro
por cada editor de painel com alterações por guardar. A pré-visualização e as informações do
ficheiro precisam do outro painel, por isso esperam até que o editor dele seja fechado.

O visualizador e o editor procuram texto da mesma forma. No visualizador, `/`, F7 ou Ctrl-F abrem
uma barra de pesquisa na linha de estado; no editor, é Ctrl-F ou F7. O Borg procura à medida que
escreve, a partir do ponto onde a barra abriu: todas as correspondências no ecrã ficam marcadas, a
atual destaca-se, e a barra conta-as (3 de 17). Enter, Down ou F3 vão para a correspondência
seguinte, e Shift-Enter, Up ou Shift-F3 para a anterior; depois da última correspondência, a
pesquisa continua a partir da outra ponta e avisa disso. Alt-C distingue maiúsculas de minúsculas
(caso contrário, uma pesquisa em minúsculas encontra qualquer caixa), Alt-W procura palavras
inteiras, e Alt-R lê a pesquisa como expressão regular; também pode clicar nas opções. Ctrl-P e
Ctrl-N trazem de volta pesquisas recentes. Esc fecha a barra; a seguir, `n` e `N` no visualizador,
ou F3 e Shift-F3 no editor, percorrem as correspondências. Num ficheiro grande, o visualizador
procura em segundo plano, mostra até onde chegou e para quando prime Esc. Conta as
correspondências em ficheiros até 64 MiB, até 10 000 delas.

## Configuração

Mude o idioma da interface em **F9 → Opções → Idioma**. Escolha inglês, chinês simplificado,
português (Portugal), espanhol ou russo e prima Enter; Esc cancela. A escolha fica guardada em
`config.toml` como `language = "en"`, `"zh-CN"`, `"pt"`, `"es"` ou `"ru"`.
`BORG_LANGUAGE` sobrepõe-se à definição guardada. Se nenhum estiver definido, o Borg segue
`LC_ALL`,
`LC_MESSAGES` ou `LANG`, e usa inglês para os idiomas que não suporta. Os comandos, os caminhos,
os nomes de ficheiros e os atalhos de teclado nunca são traduzidos. O menu de idioma deste site
muda o idioma do guia de forma independente.

As definições ficam guardadas em `~/.config/borg-commander/config.toml`, ou na pasta indicada por
`$XDG_CONFIG_HOME` ou `$BORG_CONFIG_DIR`. F9 → Opções → Definições mostra-as como caixas de
verificação; Space ou um clique alterna uma definição e guarda-a de imediato. Esc fecha a janela.

```toml
theme = "neon"              # midnight, aurora, neon, classic, paper, ember, or your own
icon-style = "auto"         # auto, nerd, or portable; Ghostty gets rich icons automatically
icon-spacing = 1            # spaces after an icon, 1 to 3
status-line = true
menu-bar = false            # keep the menu bar visible
show-hidden = false
return-to-panels = true    # return after commands launched from the panels
restore-terminal = false    # bring back the shell scrollback on the next start
screensaver = true          # a spinning cube or a glitching logo, picked at random
screensaver-after = 300     # seconds of idle time
update-check = true         # look for a new release once a day
graphics = "auto"           # auto, kitty, sixel, iterm or off
relay = "wss://relay.example.com"  # the relay for borg cloud

[plugins]
git = true
system = true
docker = true
docker-console = true       # the command line follows container panels into the container
agent-log = true            # log what Claude does while Borg runs
agent-cloud = true          # paired computers reach this borg through borg cloud
agent-ask-reads = false     # reads from another computer wait for approval

[search]
exclude = ["*.min.js", "fixtures/large"]  # paths below the search folder, as in .gitignore
artifacts = true            # leave out node_modules, Cargo's target and cache-tagged folders
ignore-files = true         # follow .gitignore, .ignore, .rgignore and .borgignore
history = true              # remember searches for Ctrl-P and Ctrl-N

[index]
roots = ["~/Projects"]      # folders indexed for instant search
refresh-minutes = 15        # rebuild an index once it is this old
```

Num terminal local com cor verdadeira, o Borg arranca com uma breve animação do logótipo com
efeito de falha (glitch); qualquer tecla ou clique a salta. Por SSH, e em terminais sem cor
verdadeira, abre diretamente nos painéis.

As pastas dos painéis, as posições do cursor, a divisão, os modos de listagem e a ordenação ficam
guardados em
`~/.local/state/borg-commander`, ou na pasta indicada por `$XDG_STATE_HOME` ou `$BORG_STATE_DIR`.

Enquanto corre, o Borg guarda os scripts de integração com a shell e os ficheiros copiados de
contentores em
`$XDG_RUNTIME_DIR/borg-commander`, ou em `borg-commander-<uid>` na pasta temporária. Só o
utilizador pode abrir esta pasta; se já existir uma criada por outra pessoa, ou em que outros
utilizadores possam entrar, o Borg ignora-a e usa outro nome. O ficheiro `TTL` começa com a hora
até à qual a pasta está em uso: cada borg em execução adia essa hora um dia, de hora a hora, pelo
que, depois de ela passar, a pasta pode ser eliminada.

Os temas personalizados ficam em `~/.config/borg-commander/themes/`. Um tema pode estender um dos
temas incluídos:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

Para compilar, testar ou contribuir, consulte [Desenvolvimento](../../../DEVELOPMENT.md).

## Uma pequena distração

Abra Sobre com F1 e escreva `2048` para jogar numa janela. As setas, ou um clique na direção de um
dos lados do tabuleiro, movem as peças; U desfaz a última jogada, R começa um jogo novo e Esc fecha
a janela. Quando chegar a 2048, prima Enter para continuar a jogar. O tabuleiro e a sua melhor
pontuação são guardados entre sessões.

## Atividade e memória

**Alt-M** ou **Comando → Monitor de atividade** abre o monitor de processos. Faz parte do plugin
Sistema incluído, que está ativo por omissão e pode ser desativado em Opções → Plugins. A lista de
processos ocupa o painel ativo, e o outro painel mostra um resumo da memória do processo
selecionado. **Enter** ou um duplo clique abre esse processo numa janela sobre o monitor, com o
mapa de memória de um lado e uma pré-visualização dos bytes da região selecionada do outro.
**Esc** fecha-a e **Tab** alterna entre os dois lados. **Alt-W** ou o botão **[+]** maximiza o
painel com o foco, **[=]** restaura a divisão e **[-]** oculta o monitor. O monitor recolhe
amostras uma vez por segundo enquanto está aberto e para quando o fecha. A amostragem fica em
pausa enquanto decorre uma inspeção de memória e é retomada quando chega o resultado.

A lista mostra CPU, memória residente, crescimento da memória e débitos de leitura e escrita em
disco. CPU a 100% corresponde a um núcleo totalmente ocupado. As páginas residentes partilhadas
entre processos contam para cada um deles, pelo que somar a RAM de todos os processos não dá o
total do sistema. Os débitos de CPU e de disco aparecem a partir da segunda amostra.

| Tecla | Ação |
| --- | --- |
| Enter | Abrir o mapa de memória do processo / passar para a pré-visualização da região |
| `/` | Filtrar por nome ou PID; ↑↓ continuam a mover, Enter mantém o filtro, Esc limpa-o |
| C / M / G / N / P | Ordenar por CPU / RAM / crescimento / nome / PID; prima de novo para inverter |
| I / O | Ordenar por débito de leitura / escrita |
| T | Mostrar ou ocultar a árvore de processos |
| Space | Pausar ou retomar a amostragem |
| R | Atualizar a vista atual |
| Esc | Voltar; na lista de processos, fechar o monitor |

Clique no cabeçalho de uma coluna para ordenar, e faça duplo clique num processo para abrir o seu
mapa de memória. O resumo mostra a memória residente atual, o valor mais alto observado, a
variação desde o início da monitorização, a CPU, o espaço de endereçamento virtual e um gráfico da
memória residente nos últimos 60 segundos. Os totais de memória usada, em swap, privada e
partilhada aparecem depois de o mapa de memória ser lido, se o sistema os indicar. **Enter** no
painel de memória abre o mapa de memória. O mapa só é lido quando o pede; prima R para o voltar a
ler depois de o processo alterar os seus mapeamentos.

### Mapa de memória

O mapa tem três colunas:

- **Memória usada** é a memória imputada apenas a este processo: as páginas que escreveu, mais as
  páginas enviadas para swap ou comprimidas. É o valor a vigiar para detetar fugas de memória e,
  no macOS, coincide com a coluna *Memória* do Monitor de Atividade.
- **Residente** são as páginas que estão neste momento na RAM, incluindo as partilhadas com outros
  processos.
- **Mapeada** é espaço de endereçamento, incluindo reservas. Não diz nada sobre o uso de RAM.

Um processo pode mapear gigabytes, sobretudo bibliotecas do sistema partilhadas com todos os
outros processos, e ter uma memória usada de apenas alguns megabytes. Inicialmente, as regiões
aparecem agrupadas por finalidade: heap e alocações, pilhas das threads, código e dados do próprio
programa, bibliotecas partilhadas, ficheiros mapeados, memória partilhada, alocações do sistema e
outras, e intervalos reservados e páginas de guarda. Os grupos e as regiões dentro deles são
ordenados pela memória usada, com as reservas no fim. As bibliotecas e os ficheiros mapeados têm
uma linha por ficheiro; expanda um ficheiro para ver as suas regiões. Os nomes dos grupos são
deduzidos das etiquetas atribuídas pelo sistema operativo. Selecionar um grupo ou ficheiro
explica o que é e mostra a sua parte da memória usada e os seus maiores membros. Enter ou Right
expandem um grupo ou ficheiro; Left recolhe-o, ou recolhe o que contém a linha selecionada. G
alterna entre a lista agrupada e a lista em bruto.

### Ficheiros abertos

**1 Memória** e **2 Ficheiros abertos** alternam entre os separadores de inspeção; também pode
clicar neles. A lista de ficheiros abertos mostra cada descritor, o modo de acesso, o tipo e o
caminho ou o ponto de rede; o outro painel mostra os detalhes completos, o tamanho do ficheiro e o
deslocamento atual, quando estão disponíveis. No macOS inclui também entradas como `cwd`
(diretório atual) e `txt` (executável mapeado). R atualiza a lista. Cada separador mantém a sua
posição na lista quando muda de separador. Quando o acesso é restrito, o Borg propõe tentar de
novo como administrador. Num ficheiro normal, os detalhes mostram também uma pré-visualização do
conteúdo, como a pré-visualização dos painéis: **Tab** entra na pré-visualização para a poder
percorrer, e **F3** abre o ficheiro no visualizador. O Borg abre o ficheiro pelo caminho, com as
suas próprias permissões, e deixa o descritor do processo em paz.

### Pré-visualização de regiões

Quando seleciona uma região legível, o Borg captura até 64 KiB dela no outro painel. Se o início
da região contiver apenas zeros, a captura avança até à primeira página com dados, e a linha por
cima dos bytes indica em que ponto da região está a janela. Quando se move depressa, o Borg espera
um momento antes de ler; selecionar outra região limpa a pré-visualização anterior e cancela a
leitura pendente. As regiões que não podem ser lidas mostram uma mensagem. Prima **Tab** ou
**Enter** para percorrer a pré-visualização. **Esc** aí volta à lista, e **Esc** no mapa volta à
lista de processos com o cursor e o filtro como estavam.

As sequências de linhas a zero recolhem-se numa única linha, como "· 60K bytes a zero ·". **]** e
**[** saltam para o bloco de dados seguinte e anterior, e **>** e **<** capturam os 64 KiB
seguintes e anteriores da região. **H** alterna entre hexadecimal e texto, **R** volta a capturar
a janela mostrada e **D** realça o que mudou desde a captura anterior do mesmo processo e
endereço. A vista de texto mostra ASCII imprimível e substitui os outros bytes por pontos. As
capturas são feitas sobre o processo em execução sem o pausar, pelo que uma captura não é uma
imagem consistente de todo o processo. Ficam em memória, a menos que as exporte. **E** guarda os
bytes em bruto num ficheiro novo na pasta do painel de ficheiros ativo, legível apenas pelo
utilizador. Os ficheiros existentes nunca são substituídos.

**M** alterna a pré-visualização entre bytes e um mapa de páginas da região: uma célula por
página de 4 KiB, ou por sequência de páginas numa região grande, assinalada conforme o que
contém: ponteiros para a memória do próprio processo, texto, texto UTF-16, código, dados
comprimidos ou cifrados, outros dados, enchimento, zeros e páginas que não estão em memória. Uma
legenda mostra a proporção de cada tipo na região. Com o mapa em foco, as setas, **PgUp**,
**PgDn**, **Home** e **End** movem um cursor, e **Enter** abre como bytes a página sob o cursor.
As regiões com mais de 16 384 páginas são amostradas de forma uniforme. No Linux, o mapa pergunta
ao sistema que páginas estão em memória e não lê as restantes, pelo que não traz de volta páginas
em swap nem toca em páginas novas; no macOS, as páginas nunca usadas são lidas como zeros. Os
tipos são deduzidos apenas a partir dos bytes, por isso encare-os como uma
indicação.

### Pesquisa de memória

Prima **/** no painel de memória, ou **Ctrl-F** em qualquer dos painéis, para pesquisar a
memória. Escolha um modo, se as maiúsculas e minúsculas têm de coincidir, e se quer pesquisar a
memória legível do processo ou a captura atual. **Texto** pesquisa UTF-8, UTF-16 (nas duas ordens
de bytes) e UTF-32 (nas duas ordens de bytes). **Unicode** normaliza também os caracteres de
compatibilidade e as sequências combinadas: por exemplo,
`𝐁𝐨𝐫𝐠` corresponde a `Borg`. Não trata como iguais letras diferentes só por serem parecidas.
**Bytes hexadecimais** aceita pares de bytes como `42 6F 72 67`.

**Cadeias** e **Indicadores** só leem a memória que o próprio processo preencheu: memória que não
provém de um ficheiro e em que o processo pode escrever; no macOS, as regiões em que nunca
escreveu também são ignoradas. **Cadeias** lista uma vez cada sequência de pelo menos seis
caracteres ASCII imprimíveis ou UTF-16LE, com o número de cópias em memória, e ignora enchimento
como caracteres ou pontuação repetidos; se indicar uma pesquisa, mantém apenas as cadeias que a
contêm. **Indicadores** mantém as cadeias que parecem um URL, um endereço IP, um endereço de
email, um caminho de ficheiro, uma linha de comando, um segredo (uma chave privada, um token de
acesso ou a atribuição de uma palavra-passe) ou uma sequência longa de base64, e classifica cada
uma.

O Borg lê também as cadeias dos ficheiros que o processo mapeou, até 1 GiB, e assinala com • as
cadeias encontradas apenas em memória: texto que o processo construiu, recebeu ou descodificou e que
nenhum dos seus ficheiros contém. Os indicadores aparecem primeiro, depois as cadeias
encontradas apenas em memória e, por fim, as que também existem nos ficheiros; dentro de cada
grupo, as cadeias que parecem palavras vêm antes das restantes. A lista guarda as 2000 melhores.
Enquanto as outras correspondências mostram a sua região, aqui cada linha mostra o tipo e o número
de cópias, e a pré-visualização mostra a cadeia no seu contexto; **C** percorre as cópias. Encare
estes resultados como pistas que merecem um olhar mais atento. Os programas comuns também
contêm muitos URL, caminhos e chaves.

As correspondências ocupam o painel da lista e mostram o endereço, a codificação e a região.
Selecionar uma correspondência pré-visualiza os bytes à volta dela no outro painel, com a
correspondência realçada. Enter entra na pré-visualização e Esc volta às correspondências. Esc
nas correspondências regressa ao mapa de memória ou à lista de processos de onde veio. Pesquisar
uma captura guardada usa os bytes dela sem ler o processo em execução. As pesquisas em processos
leem a memória por partes, indicam os bytes que não conseguiram ler e param ao fim de 2000
correspondências ou 60 segundos. **N** continua a partir do ponto onde a pesquisa parou, e
**Ctrl-C** interrompe-a e mantém os resultados. O relatório de cobertura distingue os bytes
pesquisados dos ilegíveis. Um processo pode alterar a sua memória enquanto está a ser pesquisado.

### Verificação de integridade do código

**I**, no mapa de memória ou nas correspondências de uma pesquisa, verifica se o processo está a
executar o código que os seus ficheiros contêm. Compara o código em memória com os ficheiros a
partir dos quais foi mapeado e indica as páginas que diferem, procura cabeçalhos de imagens
executáveis (ELF, Mach-O e PE) na memória que não provém de ficheiros, e lista a memória
executável que não provém de ficheiros, a memória que é simultaneamente gravável e executável, e o
código de ficheiros eliminados, sem nome ou temporários. Verifica também o processo no seu todo:
bibliotecas injetadas através de `LD_PRELOAD`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`
ou `/etc/ld.so.preload`, caminhos de pesquisa de bibliotecas, um tracer no Linux, um programa cujo
ficheiro foi eliminado ou substituído, e um nome de processo diferente do nome do programa.

As conclusões ocupam o painel da lista, com os alertas primeiro, depois os avisos e por fim as
notas. Selecionar uma pré-visualiza os bytes a que se refere, com as provas realçadas, e as linhas
por baixo da lista indicam o que mais a poderia explicar. Os programas que compilam código
enquanto correm, como os browsers e os motores de Java e JavaScript, contêm sempre código que não
provém de nenhum ficheiro, por isso, nesses casos, essas conclusões são apenas notas. **R**
verifica de novo, **Ctrl-C** interrompe uma verificação e mantém o que encontrou até então, e
**Esc** volta atrás. Uma verificação lê no máximo 512 MiB de código e 1 GiB de outra memória,
durante até 45 segundos, e indica se parou antes do fim.

No Linux, as tabelas de páginas mostram que páginas de código um processo alterou, pelo que só
essas são lidas; no macOS, as regiões sem páginas modificadas também não são lidas. O código da
cache partilhada do macOS não tem um ficheiro próprio e não é comparado. A memória que uma
verificação não consegue ler é contabilizada, e o Borg oferece **A: Administrador**. Encare as
conclusões como pistas, porque depuradores, profilers, atualizadores e ferramentas de segurança
deixam os mesmos vestígios.

### Acesso de administrador

Quando a leitura de um mapa de memória ou de uma captura está restrita, ou uma pesquisa não
conseguiu ler parte da memória, o Borg oferece **A: Administrador**. No macOS, isto abre o pedido
de autorização do sistema; no Linux, usa `pkexec` e precisa de um agente de autorização em
execução. Apenas um auxiliar de inspeção de curta duração corre com privilégios elevados. O
próprio Borg continua a correr normalmente, e o auxiliar devolve apenas dados do mapa ou um número
limitado de bytes.

O macOS protege a memória dos seus próprios programas (System Integrity Protection) e dos programas
compilados com o Hardened Runtime contra todos os outros programas, incluindo os administradores.
O Borg obtém esta informação a partir da assinatura do programa: os detalhes do processo indicam
**Memória protegida**, uma leitura recusada explica porquê, e **A: Administrador** não é oferecido,
porque não ajudaria. Os mapas de memória e os ficheiros abertos destes programas continuam a ser
mostrados. Também não é oferecido quando o Borg já está a correr como administrador. No Linux, a
política de segurança pode ainda assim negar o acesso; o Borg mostra o erro e o monitor de
processos continua a funcionar normalmente.

Um processo é identificado em conjunto com a sua hora de início, pelo que um processo novo que
reutilize um PID não herda o histórico de outro processo. Os mapas de memória, os resultados de
inspeção, as capturas e o histórico têm todos limites de tamanho. O nome Borg Commander fica
visível no canto inferior direito em todo o lado, incluindo a consola, o visualizador, o monitor,
as caixas de diálogo e a proteção de ecrã.
