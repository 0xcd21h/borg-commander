# Borg guia do utilizador

[De volta à montra](../../../README.md)

[Instalar](#install) · [Temas e ícones](#themes-and-icons) · [Teclas](#keys) ·
[Rato e consola](#mouse-and-console) · [Plug-in Git](#git-plugin) ·
[Encontrar, tarefas e visualizadores](#find-jobs-and-viewers) · [Configuração](#configuration)

## Instalar

Descarregue um binário de [GitHub Releases](https://github.com/0xcd21h/borg-commander/releases) ou execute o instalador em macOS ou Linux, com processador Intel ou ARM:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

O instalador verifica a soma de verificação do arquivo e instala `borg` num diretório do PATH, para funcionar no mesmo terminal: usa `~/.local/bin` quando já está no PATH; `/usr/local/bin` quando é gravável, por exemplo como root num contentor; ou, em alternativa, `~/.local/bin`, que adiciona ao ficheiro de arranque da shell. Depois inicia o Borg. `BORG_NO_RUN=1` impede esse arranque e `BORG_INSTALL_DIR` permite escolher o diretório. Se a mesma versão já estiver instalada, uma nova execução dispensa o download. Para compilar a partir do código-fonte, utilize Rust 1.97 ou mais recente:

```sh
cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

O suporte para Homebrew está preparado. Quando o repositório tap estiver publicado:

```sh
brew install 0xcd21h/tap/borg-commander
```

Execute `borg` na pasta que pretende explorar. F10 sai; Ctrl-O regressa da consola aos painéis. Execute `borg update` para atualizar uma instalação descarregada, ou utilize o gestor de pacotes se instalou através de Homebrew ou cargo. `borg --version` mostra a versão instalada.

## Temas e ícones

Escolha um tema com F9 → Opções → Tema. Veja o [galeria temática](../../../README.md#themes) para Midnight, Aurora, Neon, Classic, Paper e Ember.

Os temas utilizam cores fixas para que as personalizações da paleta do terminal não alterem o seu aspeto. Os terminais sem truecolour usam as cores mais próximas na paleta 256-colour.

Os ícones dos ficheiros são sempre mostrados. Ghostty utiliza os ícones Nerd Font mais ricos automaticamente. Outros terminais utilizam símbolos portáteis como padrão: `/` para pastas, `@` para links e `*` para executáveis. Definir `icon-style = "nerd"` utilizar ícones ricos noutro terminal com uma fonte adequada, ou `icon-style = "portable"` para forçar símbolos portáteis. O padrão, `"auto"`, adapta-se ao terminal sem guardar esta deteção como preferência de fonte.

Os ficheiros de lançamento incluem **Borg Mono**, uma fonte monoespaçada com ícones Nerd Font. Instale as fontes normais e a negrito de `fonts/`, selecione Borg Mono no seu terminal e escolha o `"nerd"` estilo de ícone. Borg não é possível alterar a fonte do seu terminal. O instalador guarda-os em `${XDG_DATA_HOME:-~/.local/share}/borg-commander/fonts`; Homebrew mantém-nos dentro `$(brew --prefix)/share/borg-commander/fonts`.

## Colecções e pesquisa recursiva

O menu Pesquisar abre coleções e pesquisa recursiva de nomes de ficheiros. Ambos utilizam resultados classificáveis ​​com caminhos de origem e o visualizador existente e visualização rápida.

- **Ctrl-F:** pesquisa recursivamente abaixo da pasta atual. Todas as palavras têm de corresponder: `main rs` encontra `main.rs`, e `a | b` encontra qualquer uma das duas. Uma palavra em minúsculas ignora maiúsculas e minúsculas; uma com alguma maiúscula respeita-as. Também funcionam globs como `*.log`, frases entre aspas, caminhos com barra como `src/lib` e `-word` para excluir correspondências, bem como os filtros `ext:rs,toml`, `type:file`, `type:dir`, `type:link`, `size:>10M`, `size:1M..50M`, `mod:<7d` (alterado na última semana), `mod:>1y` e `in:src` (uma pasta em qualquer ponto do caminho). As pesquisas mais recentes aparecem por baixo do formulário, primeiro as feitas na mesma pasta; clique numa, ou prima Ctrl-P e Ctrl-N no campo da pesquisa, para a usar. Os nomes correspondem em qualquer alfabeto, seja qual for a capitalização de um glob, e as letras acentuadas correspondem seja qual for a forma como o sistema de ficheiros as guarda. As correspondências perto da pasta da pesquisa aparecem primeiro, enquanto o resto da árvore ainda está a ser lido.
- **Alt-K:** recolha os ficheiros marcados, ou o ficheiro atual, no outro painel. Adicionar uma referência não copia o conteúdo do ficheiro.
- **Alt-V:** escolha uma coleção guardada numa lista rolável com contagens de entradas e estimativas de tamanho. Prima F7 ou N para criar um. As recolhas sobrevivem ao reinício Borg.
- **Esc:** ocultar resultados, preservando o cursor, o scroll, as marcas, o filtro e o agrupamento.
- **Alt-Shift-R:** retome a visualização oculta. Uma verificação continua enquanto está oculta.
- **Alt-D — Tamanhos de pasta:** encontrar pastas pelo tamanho total ou contagem de ficheiros, incluindo ficheiros ocultos por defeito. Abre a caixa de diálogo Ctrl-F definida como *Totais de pastas*; trocando   *Procure* se move entre os dois. Ctrl-C interrompe uma verificação.
- **Alt-S:** calcular tamanhos para a pasta selecionada. **Alt-Shift-D:** abra uma árvore de tamanhos para a pasta atual.
- **Alt-W:** maximizar ou restaurar o painel de resultados ativo. Os resultados são abertos num painel normal por defeito. A barra de título `[+]` (maximizar), `[=]` (restaurar) e `[-]` (dobrar) os controlos são clicáveis. Fold oculta a visualização; Alt-Shift-R retoma com o seu layout e seleção preservados.
- **Ctrl-S:** filtre os resultados por nome ou caminho de origem. Um filtro vazio mostra tudo.

Clique no cabeçalho de uma coluna para a ordenar ou reverter. Os menus Esquerda/Direita também oferecem classificação; A pesquisa adiciona a classificação e o agrupamento de pastas de origem por pasta ou tipo de ficheiro. Enter num título de grupo dobra-o; O Insert marca os seus membros correspondentes. As marcas sobrevivem à classificação e filtragem; o rodapé também conta os ficheiros marcados ocultos pelo filtro.

F3 e Ctrl-Q abrem a fonte real. F5 cópias para um destino real; se o painel de destino for uma coleção, adiciona referências. F6 requer um destino real. F8 remove referências de uma coleção. Nos resultados da pesquisa, F8 pede para apagar os originais. Shift-F8 também exclui originais de uma coleção, após confirmação. Quando uma pasta e o seu conteúdo são marcados, a pasta é enviada uma vez para o trabalho. O Reveal original abre a sua pasta de origem no outro painel. Esquerda ou Esc oculta resultados; Alt-Shift-R retoma-os no mesmo lugar. Tab alterna os painéis virtuais mesmo quando o shell tem um comando pendente. O shell mantém um diretório de trabalho real. F10 pergunta sempre antes de desistir.

A pesquisa lê nomes de ficheiros e metadados, com um trabalhador cancelável e um máximo de 20.000 correspondências por pesquisa. A linha de contexto mostra a conclusão, os erros, as entradas ignoradas e se este limite foi atingido. As pastas ocultas seguem Mostrar ficheiros ocultos. A pesquisa permanece no sistema de ficheiros inicial e não recorre através de links simbólicos de diretório. A janela de pesquisa pode restringir os resultados a ficheiros ou pastas e definir um tamanho mínimo. Escolha **Totais de pastas** para medir todos os ficheiros descendentes antes de filtrar por nome, tamanho ou contagem mínima de ficheiros. Mantém as maiores 20.000 pastas correspondentes e classifica por tamanho de conteúdo; clique em Ficheiros para ordenar por número de ficheiros. Utilize um padrão como `node_modules` ou `*cache*`, ou `*` para inspecionar tudo o que está abaixo da raiz escolhida. Os totais contabilizam o conteúdo do ficheiro, sem garantia de espaço recuperável em disco. As linhas da pasta aninhadas sobrepõem-se. Os links simbólicos nunca são seguidos, as varreduras permanecem no sistema de ficheiros inicial e uma pasta que se contém a si própria (através de uma montagem de ligação ou de uma pasta com ligação física) é inserida apenas uma vez; tais limites e as crianças ilegíveis ou excluídas tornam os totais incompletos, apresentados com ≥. As verificações de diretório utilizam metadados e não leem o conteúdo do ficheiro. Os resultados aparecem após a medição; a verificação permanece cancelável. **Árvore de tamanho de pasta** mostra o total raiz e as suas subpastas no painel atual. À direita expande uma ramificação ou entra numa ramificação expandida; Esquerda dobra ou seleciona o seu pai. Enter abre a pasta real. Clique numa seta de ramificação para duplicar ou expandir, ou num cabeçalho de coluna para classificar irmãos por nome, tamanho ou contagem de ficheiros. Esc esconde a árvore; Alt-Shift-R restaura as suas ramificações, marcas e cursor. As operações de ficheiro utilizam caminhos reais e a marcação de pai e filho envia apenas o pai para uma tarefa de cópia ou eliminação.

As pesquisas por nome omitem as pastas geradas enquanto **Omitir resultados de compilação, caches e ficheiros ignorados** estiver marcado: `node_modules`, `.git`, caches e ambientes virtuais de Python, pastas como `target` e `dist` quando há um ficheiro de compilação ao lado (`Cargo.toml`, `package.json`), e qualquer pasta marcada com um ficheiro `CACHEDIR.TAG`. Os ficheiros que `.gitignore`, `.ignore`, `.rgignore` ou `.borgignore` excluem também são omitidos, tal como os globs em `[search] exclude`. Prima F4 no diálogo de pesquisa, ou clique em **F4 Editar**, para mudar o que é omitido: ligue ou desligue as pastas geradas e os ficheiros ignorados, e adicione (Ins), edite (Enter) ou remova (Del) padrões. As alterações ficam guardadas no ficheiro de configuração. Uma pasta omitida cujo próprio nome corresponda continua a aparecer, e a linha de contexto conta as pastas omitidas. `excluded:yes` pesquisa tudo uma vez, e `hidden:yes` inclui ficheiros ocultos. As análises de tamanho leem sempre tudo. No macOS, as pesquisas e as análises de tamanho iniciadas acima omitem `~/Library/Containers` e `~/Library/Group Containers`, onde o macOS retém cada leitura até alguém responder se permite o acesso aos dados de outra app; uma pesquisa iniciada numa delas lê-a. O histórico de pesquisa fica neste computador e só o utilizador o pode ler; `history = false` em `[search]` deixa de o guardar.

**Pesquisar → Indexar esta pasta para pesquisa instantânea** mantém um índice de todos os nomes abaixo da pasta ativa, para que Ctrl-F nela, ou em qualquer pasta abaixo, responda de imediato; a linha de contexto indica quando o índice foi criado. Os índices são reconstruídos em segundo plano depois de o Borg iniciar e quando têm mais de 15 minutos, e o índice anterior responde até o novo estar completo. Uma pesquisa que não omite nada, ou que usa outras exclusões, percorre as pastas. Escolha novamente o item para deixar de indexar e apagar o índice. Os índices ficam na pasta de cache com as análises de tamanho; `[index]` na configuração lista as pastas indexadas.

Para medir tamanhos enquanto mantém os painéis de ficheiros comuns abertos, prima **Alt-I** ou escolha **Pesquisar → Contar tamanhos em segundo plano**. Borg verifica todas as pastas abaixo do diretório atual, inclui ficheiros ocultos e preenche os totais das pastas à medida que terminam. Pode navegar ou usar a consola enquanto ele é executado. As pastas que ainda estão a ser medidas mostram um total em execução crescente marcado `≥`e os totais permanecem quando sai de uma pasta e regressa. O rodapé do painel mostra o progresso; **Alt-I** novamente, ou um clique no rodapé, interrompe a digitalização e mantém os totais medidos até à data. Iniciar um trabalho de cópia, movimentação ou eliminação interrompe uma verificação de tamanho em curso. As varreduras concluídas partilham a sua cache com a árvore de tamanho e a análise de espaço.

As verificações concluídas são guardadas localmente. Tanto a análise de espaço como a árvore de pastas reutilizam uma verificação guardada para a mesma política de raiz e de ficheiros ocultos. A reabertura de uma raiz medida utiliza este instantâneo imediatamente: `~` marca os tamanhos armazenados em cache, com a data de medição acima da árvore. **Ctrl-R** verifica novamente em qualquer visualização de tamanho; **Ctrl-C** cancela. A árvore anterior permanece visível durante uma atualização e sobrevive ao cancelamento ou falha. As listagens de pastas comuns também mostram os totais em cache disponíveis. Alterações fora de Borg podem torná-los desatualizados; a navegação não aciona varrimentos recursivos. Borg copiar, mover e eliminar tarefas invalidam os instantâneos que se sobrepõem aos seus caminhos alterados.

O índice SQLite local retém cada pasta medida até 16 raízes. As árvores transportam as crianças à medida que os ramos se expandem; uma ramificação apresenta até 20.000 pastas e a análise de espaço plana mantém as maiores 20.000 correspondências. Estes limites de visualização não interrompem a digitalização nem reduzem os totais medidos. Uma linha “Ficheiros nesta pasta” contabiliza os bytes armazenados diretamente numa pasta expandida. Os erros e as subárvores ignoradas marcam os totais como incompletos. Os ficheiros de cache residem em `~/.cache/borg-commander`, respeitando `XDG_CACHE_HOME`; `BORG_CACHE_DIR` substitui a localização. São medidas descartáveis, separadas das coleções. Os ficheiros com link físico contam uma vez por caminho. Estes são tamanhos de conteúdo lógico, não de utilização de disco físico ou recuperável.

As ações Git e de ficheiro funcionam atualmente em painéis de diretório reais. Utilize o Reveal original para aceder a eles para um ficheiro recolhido.

## Teclas

A barra de atalho de duas linhas encontra-se abaixo da linha de comandos. As teclas de função estão na primeira linha; pesquisa, coleções e outros atalhos contextuais estão no segundo. Ambas as linhas são clicáveis. Os avisos substituem temporariamente a segunda linha.

| Chave | Ação |
| --- | --- |
| Cima, Baixo, PgUp, PgDn, Home, End | Mover |
| Enter, Certo | Abrir pasta/executar ou visualizar ficheiro |
| Esquerda | Vá para a pasta pai |
| Tab | Alternar painel ou focar na visualização rápida/regressar à lista de ficheiros |
| Ctrl-O | Mostrar a shell/voltar aos painéis |
| Alt-I | Contar o tamanho das pastas em segundo plano/parar de contar |
| Ctrl-S | Localização rápida na pasta |
| Ctrl-Q | Visualização rápida: o outro painel visualiza o ficheiro sob o cursor (texto, imagem ou pasta) |
| Ctrl-L | Informação: o outro painel mostra o tamanho, o modo, o proprietário, as datas e o espaço livre |
| Inserir, Ctrl-T, Shift-Up/Para baixo | Marcar ficheiros |
| `+` `-` `*` | Selecionar, desmarcar ou inverter por defeito (`*.rs`, `build/`) |
| Alt-Enter | Coloque os nomes marcados (ou o atual) na linha de comandos |
| Ctrl-Insert / Alt-Shift-Insert | Copiar os nomes marcados / os seus caminhos completos |
| Shift-Enter, Ctrl-J | Introduza uma nova linha no cursor da linha de comandos; Enter executa o comando, Ctrl-C limpa-o |
| Alt-A / Alt-Shift-A | Coloque o caminho da pasta deste painel/do outro painel na linha de comandos |
| Shift-PgUp / Shift-PgDn | Percorrer o histórico de saída da consola |
| Shift-Home / Shift-End | Saída de consola mais antiga/de volta ao prompt ao vivo |
| Ctrl-P / Ctrl-N, Ctrl-R | Histórico do shell: comando anterior/seguinte, pesquisa (manejado pelo seu shell) |
| Alt-. | Mostrar ficheiros ocultos |
| Alt-R | Recarregar o painel ativo: a sua pasta, resultados de pesquisa ou coleção |
| Alt-H | Hosts remotos: computadores emparelhados e as suas sessões |
| Alt-Shift-Left/Right | Redimensionar os painéis |
| Ctrl-U | Trocar painéis |
| Alt-J | Trabalhos: pausar ou cancelar cópias, movimentações e eliminações em execução |
| Alt-M | Monitor de atividades: processos e memória |
| Alt-C | Docker: Compor projetos e contentores |
| F1 | Sobre |
| F2 | Menu Ficheiro |
| F3 | Ver (texto ou imagem) |
| F4 / Shift-F4 | Editar no editor do Borg / com `$VISUAL` ou `$EDITOR` |
| F5 / F6 | Copiar/mover para o outro painel |
| F7 | Criar pasta |
| F8 | Excluir |
| F9 | Barra de menu |
| F10 | Sair |

Tab, Enter, Home, End, Left e Right atuam nos painéis enquanto a linha de comandos está vazia. Depois de digitar um comando, estas chaves vão para a shell. Ctrl-P, Ctrl-N e Ctrl-R utilizam o histórico de comandos da sua shell. Shift-Enter (ou Ctrl-J) insere uma nova linha sem executar o comando. Os comandos multilinha digitados e recuperados expandem a mesma área de prompt, diminuindo os painéis e mantendo o cursor visível. O shell mantém as suas chaves habituais de edição e conclusão.

As cópias chegam à área de transferência do seu computador através do terminal, também por SSH e num espelho que controla. O Borg guarda também as suas últimas 20 cópias: Ctrl-V ou Shift-Insert cola a mais recente nos campos do próprio Borg, como as caixas de diálogo e a pesquisa na memória, mesmo onde o terminal não chega à sua área de transferência. Na linha de comandos, Ctrl-V continua a ser do shell, por isso cole aí com a tecla de colar do seu terminal. O menu Ficheiro oferece as duas cópias para teclados sem Insert; as cópias demasiado grandes para o terminal ficam no Borg.

Se o seu terminal reservar teclas F, prima **Esc, depois um dígito** no espaço de 1,5 segundos: Esc 1–9 são F1–F9; Esc 0 é F10. Por exemplo, Esc 9 abre os menus e Esc 5 copia. Enquanto Borg espera pelo dígito, a linha das teclas de função mostra os dígitos a premir. Esc primeiro fecha o que está aberto (um aviso, resultados, visualização rápida ou Informação); um segundo Esc cancela. Alt-1–0 também funciona quando Alt chega ao terminal. Em macOS, ative Option como Meta no seu terminal, se necessário.

## Rato e consola

Clique numa linha para a focar, clique duas vezes para a abrir e clique com o botão direito do rato para a marcar. Clique na barra de menu ou nas ações inferiores para os utilizar. A roda move-se pelas entradas do painel.

Ctrl-O alterna entre painéis e consola. O shell segue a pasta do painel ativo antes de escrever ou abrir a consola. Em zsh muda de pasta silenciosamente e redesenha o prompt; em bash e fish um breve `cd` comando aparece na consola. Os comandos e Ctrl-C mantêm visível uma consola explicitamente aberta. “Regressar aos painéis após o comando” aplica-se aos comandos iniciados a partir dos painéis. No prompt da shell, Option/Alt-Left e Option/Alt-Right movem-se por palavras.

Na consola, a roda faz rolar o histórico de saída sem alterar o comando introduzido. Shift-PgUp/PgDn percorrer uma página; Shift-Home vai para a saída mais antiga; Shift-End regressa ao prompt ao vivo. A digitação também regressa ao prompt ao vivo. Utilize o gesto de seleção do seu terminal, geralmente Shift-drag, e depois o atalho de cópia normal para copiar a saída visível.

## Plug-in Git

O Git é um plugin incluído, ativado por defeito. F9 → Opções → Plugins abre a sua caixa de seleção; desligá-lo oculta o menu do Git e as colunas de estado e interrompe as verificações de estado em segundo plano. Requer Git no seu PATH.

Dentro de um repositório, os painéis mostram a ramificação e um estado de dois caracteres ao lado de cada ficheiro. O caractere à esquerda descreve o índice (alterações escalonadas); a direita descreve a árvore de trabalho. `M` significa modificado, `A` adicionado, `D` excluído, `R` renomeado, `??` não rastreado e `U` um conflito. As pastas resumem as alterações abaixo das mesmas. Os ficheiros limpos e ignorados não possuem marcador. Fora dos repositórios, os nomes dos ficheiros utilizam o espaço extra.

O menu Git oferece o estado do repositório, diferenças não preparadas e preparadas, commits recentes e preparação/desembalagem para ficheiros marcados (ou o ficheiro atual quando nada está marcado). As diferenças e o histórico são abertos no visualizador normal. O Unstage preserva os ficheiros de trabalho. O estado é atualizado após a navegação, os comandos e as ações do Git e a cada cinco segundos enquanto os painéis estão inativos. O estado de atualização no menu Git é novamente verificado imediatamente.

## Docker plug-in

**Alt-C** ou **Comando → Docker** lista os seus contentores no painel ativo, agrupados por projeto do Compose, com os contentores fora do Compose por último. O outro painel mostra o contentor selecionado: o seu estado, portas, redes, montagens, comando e política de reinicialização, com CPU ativo, memória, utilização de rede e disco e um gráfico de última hora. Num projeto mostra a pasta do projeto, os seus contentores e a sua utilização combinada. Esc fecha a visualização.

| Chave | Ação |
| --- | --- |
| ↑ ↓, PgUp, PgDn, Início, Fim | Mover |
| ← → / Enter | Dobre ou desdobre um projeto; Enter num contentor concentra os seus detalhes |
| S/X/R | Iniciar/parar/reiniciar o contentor ou os contentores do projeto |
| P | Pausar ou retomar |
| F | Navegue pelos ficheiros do contentor |
| E, Alt-2 | Abra um shell no contentor ou regresse ao shell aberto |
| Alt-1 | Mostrar novamente a visão geral do contentor, também a partir do interior do shell |
| `/` | Filtrar por nome, serviço, imagem ou projeto |
| Tab | Alternar entre a lista e os detalhes |
| Alt-W | Maximize o painel em foco |

Parando e reiniciando pergunte primeiro. Num projeto, uma ação afeta apenas os contentores que altera, pelo que Iniciar inicia os que foram interrompidos.

**F** num contentor em execução fecha a vista Docker e mostra o sistema de ficheiros do contentor nesse painel, começando no seu diretório de trabalho. O título do painel nomeia o contentor. O outro painel mantém a sua pasta host, pelo que **F5** cópias de qualquer forma, e **F3** visualizações, **F7** cria uma pasta e **F8** exclui dentro do contentor. As pesquisas, tamanhos, visualizações e edições permanecem no host. **Alt-C** regressa à visualização Docker; `..` na raiz do contentor retorna o painel para a pasta host mostrada antes. Necessidades de navegação `sh` no contentor, pelo que as imagens mínimas sem shell não podem ser listadas.

Nos ficheiros de um contentor, a linha de comando segue o painel: torna-se a shell desse contentor, iniciada na pasta do painel, com o nome do contentor no início do prompt. Os comandos são executados dentro do contentor, o painel é atualizado quando terminam e um `cd` aí se move o painel. Ctrl-O mostra a consola desse contentor, marcada **▣ consola de nome**; num painel do host, a linha de comandos e Ctrl-O são novamente do host, com tudo o que lá introduziu guardado. O shell do contentor é o mesmo do separador Shell na vista Docker.

Desligue **Docker — linha de comando segue os painéis do contentor** em Opções → Plugins para manter a linha de comandos no host. Nos ficheiros de um contentor, fica esmaecido e marcado **⌂ anfitrião**, e Enter não executa nada até que um painel host esteja ativo, pelo que um comando nunca é executado na máquina errada.

**E** abre um shell no contentor (bash quando a imagem o possui, caso contrário `sh`) no separador Shell do painel de detalhes e envia-lhe as suas chaves, incluindo as teclas de função. **Alt-1** mostra a visão geral e **Alt-C** regressa à lista; a roda do rato faz rolar o seu histórico. Um shell continua a ser executado enquanto olha para outros contentores, fecha a vista Docker ou utiliza a consola do host, e o seu contentor é marcado `›_` na lista e contou com o atalho Alt-C. Fecha quando sai, quando o seu contentor pára ou quando sai Borg, que pergunta primeiro enquanto os shells estão abertos. O plug-in utiliza o `docker` CLI e o seu contexto atual, pelo que funciona com Docker Desktop, OrbStack, Colima e motores remotos. Pode ser desativado em Opções → Plug-ins.

## Claude e outros agentes (MCP)

`borg mcp` permite que um agente de IA como o Claude trabalhe no borg que abriu, na sua máquina ou num servidor acedido por SSH. Nada precisa de ser instalado para além de borg: o agente inicia `borg mcp` em si, através de SSH para um servidor, e liga-se à sessão borg aí aberta. O servidor não necessita de porta aberta nem de acesso à Internet.

Com espaços de trabalho nomeados, `borg sessions list` mostra os seus IDs e nomes; `borg sessions create NAME` adiciona um, e `borg --session NAME` abre. `borg mcp` pode ligar `list_sessions`,
`list_remote_sessions`e `use_session` para escolher um alvo para esta ligação MCP. Uma chamada de ferramenta também pode fornecer um `session` ID para uma substituição de uma chamada. O destino selecionado permanece privado para essa ligação MCP.

`borg service install`, `enable`e `start` configure o serviço de segundo plano opcional. Possui a ligação de emparelhamento da máquina e mantém as sessões nomeadas em execução quando os seus terminais são desligados. `borg service attach NAME` se volta a ligar a um. Ctrl-] desprende-se sem acabar com a sua casca. Usar `borg service stop`, `disable`, ou `uninstall` para controlar o ciclo de vida do serviço. Num contentor sem systemd, execute o supervisor de processo do contentor `borg service run`; o `install` e `enable` os comandos requerem um host systemd em Linux.

Após o emparelhamento, `borg --remote MACHINE:SESSION` abre o sistema de ficheiros dessa sessão no painel local esquerdo. Enter ou F3 visualiza um ficheiro remoto através de um download local temporário. **F5** copia ficheiros e pastas marcados entre painéis locais e remotos ou entre dois hosts emparelhados. A caixa de diálogo de destino identifica o cliente ou o host. As cópias são executadas como tarefas de segundo plano com progresso de bytes, pausar/cancelar e substituir, ignorar ou manter ambas as opções. As pastas incluem pastas vazias e links simbólicos; permissões executáveis ​​e tempos de modificação são preservados para ficheiros regulares. Um ficheiro torna-se visível no seu destino final apenas depois de o seu conteúdo chegar completamente. F6 mover e F8 eliminar não estão disponíveis nos painéis remotos. O painel direito permanece no seu computador. Com **Opções → Plug-ins → Seguir** activado, a linha de comandos e **Ctrl-O** utilize um shell interativo no host remoto. O seu terminal é renderizado localmente; chaves, colar, Ctrl-C e programas de terminal são executados remotamente. Cada sessão remota mantém o seu próprio shell e a linha de comandos por concluir enquanto muda de painel. A seleção de um painel de cliente restaura o shell local. As aplicações que usam o ecrã alternativo e as aplicações em primeiro plano que usam entrada bruta, como `top`, receba todas as chaves durante a execução, incluindo Ctrl-O. O redimensionamento do terminal chega ao PTY remoto. Saia da aplicação usando as suas próprias teclas (`q` em `top`); Borg regressa aos painéis se o tiver iniciado a partir da linha de comandos do painel ou deixa a consola remota aberta se o tiver iniciado aí. Os próprios painéis e shell do comando Borg são independentes desta consola. Usar `borg get MACHINE/SESSION:PATH [TO]` e `borg put FROM MACHINE/SESSION:PATH` para transferências de ficheiros. No local Borg, pressione **Alt-H**, utilize a parte inferior **Hosts remotos** atalho ou escolha **F9 → Remoto → Navegar por hosts e sessões…** para ver máquinas emparelhadas e as suas sessões nomeadas. Enter abre os ficheiros da sessão selecionada no painel que abriu o seletor, substituindo a localização anterior desse painel. Pressione **M** no seletor para espelhar o seu ecrã; **R** atualiza a lista. **Tab** alterna entre **Anfitriões** (acesso de saída) e **Clientes** (acesso de entrada). Os clientes listam os computadores emparelhados com este Borg, as suas impressões digitais de chave pública, se estão ligados e as suas contagens de ligação. Um computador aparece uma vez mesmo quando um espelho utiliza diversas ligações. Atualizações de estado enquanto o seletor está aberto, incluindo dentro de uma sessão cujas ligações pertencem ao serviço em segundo plano. O emparelhamento é direccional: um cliente permitido não se torna automaticamente um host cujos ficheiros este Borg pode navegar. Quando o proprietário da ligação não consegue indicar o seu estado, os clientes emparelhados continuam visíveis com **Estado da ligação indisponível**. Enquanto um espelho se liga, o seletor mostra um estado animado e **Esc** cancela imediatamente. Cada passo da ligação tem um prazo de 10 segundos, com um limite de 30 segundos para toda a tentativa. Uma tentativa falhada mantém o seletor aberto e indica o passo que falhou; **M** tenta novamente a sessão selecionada. No selecionador, **F2** renomeia o rótulo do host selecionado para cada sessão nesse host; **Shift-F2** renomeia o rótulo da sessão selecionada. Estes rótulos são guardados neste cliente e aparecem no seletor, nos títulos dos painéis e no rodapé da consola. Deixe um nome em branco para restaurar o original. Os detalhes mantêm os nomes originais do host e da sessão. Os rótulos estão ligados à chave e ao ID da sessão do host emparelhado; os destinos e os nomes dos comandos no host remoto permanecem os mesmos.

**F8** num host indisponível esquece o seu emparelhamento neste cliente. **Shift-F8** esquece o host selecionado mesmo quando está online. A confirmação tem **Não** como predefinição. Esquecer um host fecha os painéis e as consolas deste cliente para esse host; as sessões e os ficheiros do host são mantidos. A identidade do cliente e os seus outros emparelhamentos são preservados. Emparelhe novamente para voltar a ligar.

**F8** ou **Excluir** remove a sessão do host selecionada após uma confirmação cujo padrão é **Não**. A remoção fecha essa sessão e as suas consolas remotas e remove os seus painéis Borg guardados e o histórico do terminal; ficheiros e pastas no host são mantidos. A sessão predefinida está protegida. Uma sessão autónoma que atualmente aloja a ligação não pode ser removida; feche-o no host e execute primeiro o Borg noutra sessão. A remoção falhada mantém a sessão visível para nova tentativa. As sessões geridas pelo serviço são interrompidas antes de o seu estado guardado ser removido e o serviço não as recria.

O seletor distingue os hosts de saída dos clientes de entrada; o mesmo Borg pode servir ambas as funções. O menu Remoto também oferece **Host montado em espelho** e **Partilhe isto Borg como anfitrião…**. O selector mostra a versão Borg de cada host; selecionar uma sessão mostra o sistema operativo/arquitetura, o serviço de fundo ou o modo de terminal, a latência de pedido e o endereço de retransmissão. Os hosts são verificados simultaneamente, pelo que um host indisponível não oculta os resultados de outros. Um host que não consegue indicar a sua versão mostra **versão indisponível**; a versão do relé não é a versão do host. Mantenha o Borg atualizado em ambos os computadores: um host sem uma funcionalidade remota mostra uma mensagem de atualização.

As pastas remotas chegam por partes, com um indicador de carregamento e uma contagem das entradas recebidas; a navegação cancela o pedido. `borg mirror MACHINE:SESSION` mostra o ecrã ao vivo da sessão remota no seu terminal local; Alt+Shift+M abre o mesmo espelho de um Borg local com um painel remoto. **Ctrl-G** sai do espelho e regressa aos painéis locais: mantenha premido Control e prima G. Também pode clicar na linha inferior do banner. O espelho mantém o seu estado de controlo visível na linha acima; Ctrl-] também sai. `--take` assume explicitamente o controlo de outro visualizador. Um normal `borg` ou `borg --session NAME` pode ser espelhado enquanto está a ser executado; não é necessário nenhum serviço em segundo plano. O seu terminal local permanece visível e a entrada local do teclado ou do rato recupera o controlo do espelho. O espelho segue o layout e os clipes do terminal host para caber numa janela mais pequena do cliente; ampliar o cliente revela o resto. O host de segundo plano opcional mantém as sessões ativas após o fecho do terminal original e permite ao cliente controlador redimensionar o seu PTY. Se outro terminal controlar a sessão remota, o espelho abre no modo de visualização apenas; as suas chaves não alteram a sessão remota. Pressione **Ctrl-T** ou clique na linha de estado para assumir o controlo, desligar o outro terminal ou utilizar `borg mirror MACHINE:SESSION --take` para assumir o controlo ao iniciá-lo. Depois de assumir o controlo, a primeira tecla ativa um ecrã remoto adormecido; as teclas subsequentes e a entrada do rato chegam até ele. Um visualizador que fique para trás recebe o ecrã atual em vez de fotogramas de animação antigos. Escrever `exit` no shell principal do Borg (Bash, Zsh ou Fish) pede confirmação. **Não** é selecionado por defeito e mantém o mesmo shell, variáveis, diretório e espelhos em execução. Numa consola remota, `exit` pergunta se deve fechar o shell no host; depois de Sim, escolha novamente a sessão com Alt-H para abrir outro.

Para ligar, prima F1 e depois C (ou F9 → Remoto → Partilhar isto Borg como host…). Borg mostra dois comandos para colar num terminal do seu computador: um inicia o Claude Code já ligado a este borg, o outro mantém a ligação em Claude Code para cada sessão. Tab escolhe um, Enter copia-o para a área de transferência do seu computador através do terminal, que funciona via SSH em iTerm2, Ghostty, kitty, WezTerm e Alacritty (em tmux, com `set -g set-clipboard on`). Onde o terminal não aceita, como em muitos terminais de browser, F3 mostra o comando sozinho como texto simples, com o rato livre para o seleccionar; qualquer tecla regressa a borg. Por SSH, borg preenche o endereço pelo qual entrou; se o seu computador aceder ao servidor por outro nome, como um alias SSH ou através de um host de salto, introduza esse nome. O SSH deve iniciar sessão sem pedido de palavra-passe: uma chave ou um agente SSH. `borg mcp setup prod` imprime o mesmo para o Claude Code e Claude Desktop na linha de comandos.

Ligado, o agente vê o que vê: o próprio ecrã como texto, quer esteja nos painéis ou na consola, ambos os painéis, os ficheiros destacados e marcados, onde é executada a linha de comandos e o que aí é digitado. Ela nomeia os lugares como localizações: um caminho absoluto ou `~/…` na máquina borg continua, `docker://web/etc/nginx` dentro de um contentor ou um caminho relativo ao painel ativo. Sem perguntar, lê pastas, ficheiros (por intervalos de linhas ou linhas que contêm algum texto), os processos mais ocupados e os contentores, e mostra um ficheiro ou pasta no seu outro painel.

Tudo o que altere a máquina espera por si numa caixa de diálogo que mostra tudo: um comando e onde é executado, um novo ficheiro ou uma edição como comparação. Os longos rolam com as teclas de seta, PgUp e PgDn ou com a roda do rato, e F3 mostra a caixa de diálogo em ecrã inteiro. Não é selecionado até escolher Sim; uma edição será recusada se o ficheiro for alterado enquanto estava a decidir. Para um computador emparelhado, `approval_mode` pergunta uma vez antes de ativar Aprovar tudo para essa sessão. O seu emblema permanece visível; clique no emblema ou prima Ctrl+Alt+A para o desligar imediatamente. Também é desligado quando a ligação emparelhada é fechada. As aprovações de emparelhamento permanecem separadas. Os comandos são digitados num shell que assiste e mantidos fora do seu histórico: comandos nesta máquina na própria consola do borg, que mostra enquanto são executados, para que possa responder a um prompt como o do sudo; comandos num contentor na shell desse contentor na vista Docker. O shell mantém a sua pasta e as variáveis ​​entre comandos.

O agente também pode digitar texto para que possa rever: um comando no prompt ou um script no vim ou nano aberto na consola. Nada é executado: não é premido qualquer Enter e o texto com várias linhas entra como uma pasta, que as shells e os editores inserem sem executar.

O agente também pode pesquisar a memória de um processo tal como a pesquisa de memória faz: cadeias, indicadores, texto ou bytes. A memória pode conter palavras-passe, chaves e mensagens privadas, e o que a pesquisa encontra vai para o agente, por isso a primeira pesquisa de cada processo aguarda por si numa caixa de diálogo que indica o processo, o seu programa e a pesquisa. Aprovar permite ao agente pesquisar esse processo novamente até o borg terminar. Pode pedir para ler como administrador, o que abre o pedido de palavra-passe do próprio sistema.

### Borg nuvem

Borg cloud permite que Claude alcance um borg para o qual não existe uma rota SSH: um servidor que abre num terminal de browser, um contentor, uma máquina atrás de NAT. Ambos os lados ligam-se a um relé e o relé une-os.

1. Em borg no servidor, prima F1 e depois C. A primeira opção é `borg pair` com um link; Enter cópia. O link funciona uma vez, durante dez minutos. Abaixo dele está o relé: cole lá o endereço de outro relé e pressione Enter para o guardar e obter um link através dele.
2. Execute-o num terminal do seu computador, onde também está instalado borg. Imprime um código de emparelhamento: seis emojis, cada um com o seu nome, como `🐱 Cat · 🦁 Lion · 🐎 Horse`.
3. Borg no servidor pergunta se pretende emparelhar, mostrando o nome do computador e o código. Escolha Sim se os códigos corresponderem. `borg pair` oferece-se então para adicionar o servidor ao código Claude, como    `claude mcp add borg-NAME -- borg mcp remote NAME`.

O código vem do aperto de mão encriptado que os dois borgs acabaram de finalizar, pelo que é o mesmo nos dois ecrãs apenas quando é o seu computador a falar com o servidor. Alguém que obtivesse o link e o utilizasse primeiro mostraria ao servidor um código diferente e não poderia escolher um que correspondesse ao seu. Onde um terminal desenha emojis mais estreitos do que borg os apresenta, borg mostra apenas os nomes; compare os nomes.

A partir daí, o Claude alcança esse borg sempre que lá estiver aberto. `borg pair` sozinho lista os servidores emparelhados e `borg pair --forget NAME` esquece um; no servidor, F8 no diálogo Acesso remoto esquece todos os computadores emparelhados. Através da borg nuvem Claude recebe também `download` e `upload`, que copia ficheiros entre o servidor e o seu computador; `borg get NAME:PATH [TO]` e `borg put FROM NAME:PATH` faça o mesmo no seu terminal. Os uploads aguardam a sua aprovação em borg. Cada aprovação nomeia o computador que solicitou.

A ligação é encriptada de ponta a ponta. Cada borg guarda uma chave privada, legível apenas por si, na sua pasta de estado. O emparelhamento troca as chaves públicas e cada ligação é aberta com um handshake Noise IK ligado a estas, pelo que o relé apenas reencaminha texto cifrado e não pode responder em nome de nenhum dos borg. As chamadas de outro computador podem ser lidas sem perguntar, como as locais; Opções → Plugins → "perguntar antes de outro computador ler" faz com que estas leituras também aguardem aprovação. Borg não mantém qualquer ligação com o relé, a menos que esteja emparelhado ou a mostrar uma ligação de emparelhamento, e nenhuma ligação quando borg a nuvem está desligada em Opções → Plugins.

Borg guarda o relé na configuração, como `relay = "wss://relay.example.com"`. Um computador chega ao servidor através do relé através do qual foi emparelhado; depois de o servidor mudar de retransmissão, emparelhe novamente o computador. Os borgs numa máquina partilham uma chave, pelo que apenas o primeiro aberto se liga ao relé; os outros dizem qual deles tem nuvem borg.

Enquanto um agente está ligado, **◈ Remoto** encontra-se no início da linha de atalho; clique nele para ver o registo do que fez: o que viu, leu, mostrou e digitou, e cada comando e alteração, aprovado ou recusado. Borg mantém o log apenas na memória, até sair; para não manter qualquer registo, desative-o em Opções → Plugins.

### Executar um relé

`borg-relay` é o ponto de encontro dos borgs. Num servidor Linux cujo domínio aponta para ele, com a porta 443 livre, um comando instala-o como um serviço com um certificado TLS da Let's Encrypt:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install-relay.sh | sudo sh -s -- relay.example.com you@example.com
```

Execute-o novamente para atualizar.

No Dokploy, Coolify ou outro host Docker cujo proxy adicione TLS, execute a imagem `ghcr.io/0xcd21h/borg-relay:latest` e atribua-lhe um domínio na porta 8080: no Dokploy, uma aplicação dessa imagem Docker com um domínio na porta do contentor 8080 e HTTPS ativado; no Coolify, uma funcionalidade dessa imagem Docker com a porta 8080 exposta e um domínio. Qualquer um também leva [embalagem/relay/docker-compose.yml](../../../packaging/relay/docker-compose.yml) como um serviço de composição. A imagem corre `borg-relay --behind-proxy`: o proxy serve HTTPS e passa WebSockets, e a retransmissão conta os seus limites pelo endereço do cliente no qual o proxy reporta `X-Forwarded-For`. É executado como um utilizador sem privilégios e possui uma verificação de integridade (`GET /health`).

Num host Docker sem este proxy, a mesma imagem serve o próprio TLS:

```sh
docker run -d --restart unless-stopped -p 443:443 -v borg-relay:/data ghcr.io/0xcd21h/borg-relay --domain relay.example.com --cache /data
```

Atrás de outro proxy que adicione TLS, execute `borg-relay --behind-proxy --listen 127.0.0.1:8080`; utilize-o apenas atrás de um proxy, pois o relé confia no endereço do cliente que o proxy reporta. O relé mantém limites por endereço, em servidores e sessões, em tamanho de trama, largura de banda, bytes por sessão, tempo de inatividade e duração da sessão. Não armazena nada além do seu certificado.

Os registos de retransmissão contam apenas: uma vez por minuto, quando mudam, uma linha como

```text
stats: connections=3 servers=2 sessions=1 joined=14 carried=3.2MiB limited=0
```

com as ligações que mantém, os servidores registados e as sessões abertas, e desde que foi iniciado, as sessões que se encontravam, os bytes que carregavam e as ligações e sessões com limite recusado ou terminado. Para depuração, `--log`ou a variável de ambiente `BORG_RELAY_LOG=1` para a imagem, regista também o registo de cada servidor pelo início da sua chave, cada sessão com os bytes que carregava e cada limite atingido com o endereço do cliente.

## Encontrar, tarefas e visualizadores

Ctrl-S abre a pesquisa rápida. Introduza um nome ou glob como `*.rs`; Para baixo e para cima movem-se entre correspondências, Inserir marca uma correspondência, Ctrl-A marca todas as correspondências e Enter ou Esc fecha a pesquisa. A correspondência não distingue maiúsculas de minúsculas, a menos que a consulta contenha maiúsculas.

F5, F6 e F8 iniciam a cópia, movimentação e eliminação de trabalhos. Alt-J abre a lista de trabalhos para pausar ou cancelar.

A caixa de diálogo Copiar tem uma opção opcional **Otimize a cópia para discos rígidos** caixa de seleção. Utilize Tab para focar e Espaço para alternar ou clique nele. Começa desmarcado para cada cópia. Este modo lê pequenos ficheiros na memória em lotes e depois escreve cada lote, o que pode reduzir a pesquisa de origem/destino ao copiar dentro de um HDD. Os lotes armazenam até 64 MiB de dados de ficheiro ou 4.096 ficheiros; ficheiros maiores que 1 MiB são transmitidos normalmente. O desempenho depende da unidade, do sistema de ficheiros e da cache do sistema operativo; pode ser mais lento entre unidades separadas. Para conflitos de nomes: `o` substitui, `s` salta, `k` mantém ambos, `a` substitui tudo, `n` salta tudo e Esc cancela. As cópias são gravadas em ficheiros temporários e renomeadas após a conclusão.

F3 abre o visualizador. Em ficheiros de texto, setas e scroll PgUp/PgDn, `w` altera o empacotamento e Esc ou `q` fecha. Nas imagens, Esquerda/Direita ou `p`/`n` selecione a imagem anterior ou seguinte. Os protocolos de imagem são utilizados ​​quando disponíveis; outros terminais apresentam blocos coloridos.

Ctrl-Q mostra uma visualização rápida no outro painel. Tab foca; setas, PgUp/PgDn, Home/End e a roda do rato fazem scroll no documento. Tab ou Esc regressa à lista de ficheiros. Ctrl-L mostra informações do ficheiro. F3 expande uma visualização de texto no visualizador; fechá-lo regressa à visualização com a sua posição, quebra automática e modo de origem preservados. Ambos utilizam o mesmo leitor de ficheiros limitado, pelo que a abertura de um ficheiro de texto grande é lida junto à primeira página, em vez de carregar o ficheiro inteiro.

Ambos os visualizadores de texto destacam linguagens de programação comuns, marcação e ficheiros de configuração, incluindo Rust, Python, JavaScript, TypeScript/TSX, Go, C/C++, Java, C#, Ruby, PHP, Lua, SQL, HTML/CSS, JSON, YAML, TOML, scripts de shell e Dockerfiles. O Markdown abre formatado por defeito. Pressione `s` ou F4 em qualquer visualizador para alternar a visualização de origem. São suportados títulos, ênfases, listas, tarefas, links, tabelas e blocos de código protegidos. Os ficheiros Markdown maiores que 2 MiB são abertos como origem para manter a memória limitada.

F4 abre o ficheiro selecionado no editor do Borg, e Shift-F4 em `$VISUAL` ou `$EDITOR`. O editor mantém os finais de linha e a indentação do ficheiro, destaca o código como os visualizadores e numera as linhas. Shift com as setas seleciona; Ctrl-C, Ctrl-X e Ctrl-V copiam, cortam e colam através da área de transferência do Borg, e o que se cola a partir do terminal entra tal como está. Ctrl-Z e Ctrl-Y anulam e refazem, Tab e Shift-Tab aumentam e reduzem a indentação, Ctrl-F procura (F3 novamente, Shift-F3 para trás) e Ctrl-G vai para uma linha. F2 ou Ctrl-S guarda através de um ficheiro temporário, para que o ficheiro nunca fique meio escrito, e se o ficheiro mudou no disco desde que foi aberto, pergunta se deve substituí-lo ou recarregá-lo. Esc fecha, perguntando antes pelas alterações por guardar. Os ficheiros que não são texto UTF-8, ou maiores do que 32 MiB, abrem no visualizador.

## Configuração

Altere o idioma da interface em **F9 → Opções → Idioma**. Escolha inglês, chinês simplificado, português de Portugal, espanhol ou russo e prima Enter para guardar. Esc cancela. A escolha fica em `config.toml`, como `language = "en"`, `"zh-CN"`, `"pt"`, `"es"` ou `"ru"`. `BORG_LANGUAGE` tem prioridade sobre a definição guardada. Sem nenhuma delas, o Borg consulta `LC_ALL`, `LC_MESSAGES` e `LANG`, usando inglês para idiomas não suportados. Os comandos, caminhos, nomes de ficheiros e atalhos mantêm-se. O menu de idiomas do site controla o guia de forma independente.

As definições são guardadas em `~/.config/borg-commander/config.toml`, ou o diretório selecionado pelo `$XDG_CONFIG_HOME` ou `$BORG_CONFIG_DIR`. F9 → Opções → Definições abre caixas de seleção; Espaço ou clique alterna uma definição e guarda-a imediatamente. Esc fecha a janela.

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

Num terminal local com cores verdadeiras, Borg abre com uma breve introdução do seu logótipo; qualquer tecla ou clique ignora. Sobre SSH e em terminais sem true color começa diretamente nos painéis.

As pastas do painel, a posição do cursor, a divisão, o modo de listagem e a classificação são guardados em `~/.local/state/borg-commander`, ou o local selecionado pelo `$XDG_STATE_HOME` ou `$BORG_STATE_DIR`.

Enquanto é executado, o Borg mantém os scripts de integração da shell e os ficheiros copiados dos contentores em `$XDG_RUNTIME_DIR/borg-commander`, ou `borg-commander-<uid>` na pasta temporária. Só você o pode abrir; aquele que outra pessoa criou, ou que outros possam entrar, é preterido por um novo nome. É `TTL` O ficheiro começa com a hora até à qual está a ser utilizado: cada borg em execução avança essa hora um dia a cada hora, pelo que, depois de passado, a pasta pode ser removida.

Temas personalizados entram `~/.config/borg-commander/themes/`. Um tema pode estender um tema integrado:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

Para construir, testar ou contribuir, consulte [Desenvolvimento](../../../DEVELOPMENT.md).

## Uma pequena distração

Abra Sobre com F1 e digite `2048` para jogar num modal. As teclas de seta movem-se, U desfaz o último movimento, R inicia um novo jogo e Esc fecha-o. Alcança 2048 e prime Enter para continuar a jogar. O tabuleiro e a melhor pontuação são guardados entre sessões.


## Atividade e memória

**Alt-M** ou **Comando → Monitor de atividades** abre o monitor de processo. O plugin do sistema incluído está ativado por defeito e pode ser desativado em Opções → Plugins. A lista de processos ocupa o painel ativo, com a visão geral da memória do processo selecionado ao lado. **Enter** ou um duplo clique abre este processo numa janela no monitor: o seu mapa de memória de um lado e uma visualização dos bytes da região selecionada do outro. **Esc** fecha. **Tab** muda o foco. **Alt-W** ou o **[+]** o botão maximiza o painel em foco; **[=]** restabelece a divisão e **[-]** esconde o monitor. A amostragem é executada uma vez por segundo enquanto o monitor está aberto; fechá-lo interrompe a amostragem. As inspeções de memória retêm temporariamente a amostragem até que o resultado chegue.

A lista mostra CPU, memória residente, crescimento de memória e taxas de leitura/escrita de disco. CPU 100% significa um núcleo ocupado. As páginas residentes partilhadas podem aparecer em mais do que um processo, pelo que os valores do processo RAM não devem ser adicionados como um total exclusivo do sistema. A primeira amostra CPU e a taxa de E/S ficam indisponíveis até que chegue uma segunda observação.

| Chave | Ação |
| --- | --- |
| Enter | Abra o mapa de memória do processo/focalize a visualização da região |
| `/` | Filtrar por nome ou PID; ↑↓ ainda se move, Enter mantém o filtro, Esc limpa-o |
| C/M/G/N/P | Ordenar por CPU / RAM / crescimento / nome / PID; repetir para reverter |
| E/S | Ordenar por taxa de leitura/escrita |
| T | Alternar árvore de processos |
| Espaço | Pausar ou retomar a amostragem |
| R | Atualizar a visualização atual |
| Esc | Voltar; fechar da lista de processos |

Clique nos títulos dos processos para classificar; clique duas vezes num processo para abrir o seu mapa de memória. A vista geral mostra a memória residente atual, o pico de memória observado, a alteração desde o início da monitorização, CPU, espaço de endereço virtual e um gráfico dos últimos 60 segundos de memória residente. Os totais de pegada, trocados, privados e partilhados aparecem assim que o mapa de memória é lido e a plataforma os reporta. **Enter** no painel de memória abre o seu mapa de memória. O mapa de memória é uma medição realizada mediante pedido. Atualize-o com R depois de o processo alterar os seus mapeamentos.

O mapa tem três colunas:

- **Pegada** é a memória cobrada apenas para este processo: páginas que ele escreveu mais páginas trocadas ou comprimidas. É a figura a observar em busca de fugas e a macOS corresponde ao Activity Monitor *Memória* coluna.
- **Residente** são páginas em RAM agora, incluindo páginas partilhadas com outros processos.
- **Mapeado** é o espaço de endereço, incluindo reservas. Não é RAM.

Um processo pode mapear gigabytes, principalmente bibliotecas de sistema partilhadas com todos os outros processos, enquanto o seu espaço ocupado é de alguns megabytes. As regiões começam agrupadas por finalidade: heap e alocações, thread stacks, código e dados do próprio programa, bibliotecas partilhadas, ficheiros mapeados, memória partilhada, sistema e outras alocações, bem como intervalos e guardas reservados. Os grupos e os seus membros estão ordenados por área ocupada, sendo as reservas em último lugar. As bibliotecas e os ficheiros mapeados listam uma linha por ficheiro; expanda um ficheiro para ver as suas regiões. Os nomes dos grupos são inferidos a partir dos rótulos do sistema operativo. A seleção de um grupo ou ficheiro explica-o e mostra a sua participação na área ocupada e os seus maiores membros. Enter ou Direita expande um grupo ou ficheiro; Esquerda recolhe-o ou aquele que rodeia a linha selecionada. G alterna entre listas agrupadas e em bruto.

**1 memória** e **2 Abra ficheiros** comute as guias de inspeção ou clique nos seus rótulos. A lista de ficheiros abertos inclui o descritor, o modo de acesso, o tipo e o caminho ou terminal; o outro painel mostra os detalhes completos, o tamanho do ficheiro e o deslocamento atual, quando disponível. Em macOS inclui também entradas como `cwd` (diretório atual) e `txt` (executável mapeado). R atualiza a lista. A troca de separadores mantém as posições da lista. As restrições de acesso oferecem uma nova tentativa explícita do administrador existente. Um ficheiro normal mostra também o que contém por baixo dos seus detalhes, tal como a vista rápida: **Tab** foca a pré-visualização para a percorrer e **F3** abre o ficheiro no visualizador. O Borg lê o ficheiro pelo seu caminho com as suas próprias permissões, não através do descritor do processo.

A seleção de uma região legível captura automaticamente até 64 KiB dela no painel oposto. Quando o início da região contém apenas zeros, a captura continua a ler até à primeira página com dados, e a linha acima dos bytes indica em que ponto da região está a janela. Os movimentos rápidos são agrupados com um pequeno atraso; selecionar outra região limpa a pré-visualização anterior e cancela a leitura pendente. As regiões ilegíveis mostram uma mensagem sem tentar ler. Pressione **Tab** ou **Enter** para percorrer a pré-visualização. **Esc** aí foca a lista; **Esc** a partir do mapa volta à lista de processos, mantendo o cursor e o filtro.

As sequências de linhas de zeros dobram-se numa única linha, como «· 60K bytes a zero ·». **]** e **[** saltam para o bloco de dados seguinte e anterior, e **>** e **<** capturam os 64 KiB seguintes e anteriores da região. **H** alterna a apresentação hexadecimal/texto, **R** captura novamente a janela mostrada e **D** destaca as diferenças em relação à captura anterior do mesmo processo e endereço. A apresentação de texto mostra o ASCII imprimível e substitui os pontos por outros bytes. Os instantâneos são observações ao vivo; o processo não é pausado, pelo que uma captura não é uma imagem atómica de todo o seu estado. Permanecem na memória, a menos que sejam exportados explicitamente. **E** exporta bytes brutos para o diretório do painel de ficheiros ativo, utilizando um novo nome de ficheiro e permissões de ficheiro apenas do proprietário. Os ficheiros existentes nunca são substituídos.

**M** alterna a pré-visualização entre os bytes e um mapa de páginas da região: uma célula por cada página de 4 KiB, ou por cada sequência de páginas numa região grande, marcada pelo que contém: ponteiros para a própria memória do processo, texto, texto UTF-16, código, dados comprimidos ou cifrados, outros dados, preenchimento, zeros e páginas que não estão em memória. Uma legenda indica a parte da região que cada tipo ocupa. No mapa focado, as setas, **PgUp**, **PgDn**, **Home** e **End** movem um cursor, e **Enter** abre em bytes a página que está por baixo. As regiões com mais de 16 384 páginas são amostradas de forma uniforme. No Linux, o mapa pergunta ao sistema que páginas estão em memória e deixa as restantes por ler, pelo que não traz de volta páginas trocadas nem toca em páginas novas; no macOS, as páginas não usadas são lidas como zeros. Os tipos são deduzidos apenas a partir dos bytes, por isso trate-os como uma orientação e não como um veredicto.

Pressione **/** no painel de memória ou **Ctrl-F** de qualquer painel, para pesquisar na memória. Escolha um modo, a correspondência de maiúsculas e minúsculas e a memória de processo legível ou o instantâneo atual. **Texto** pesquisa UTF-8, UTF-16 (ambas as ordens de bytes) e UTF-32 (ambas as ordens de bytes). **Unicode** também normaliza os caracteres de compatibilidade e a combinação de sequências: por ex. `𝐁𝐨𝐫𝐠` partidas `Borg`. Isto não significa letras não relacionadas que apenas parecem semelhantes. **Feitiço** aceita pares de bytes como `42 6F 72 67`.

**Cadeias** e **Indicadores** leem apenas a memória que o próprio processo preencheu: a memória que nenhum ficheiro suporta e a memória em que pode escrever; no macOS, as regiões em que nunca escreveu também ficam de fora. **Cadeias** lista uma única vez cada sequência de pelo menos seis caracteres ASCII ou UTF-16LE imprimíveis, com o número de cópias na memória, e deixa de fora o enchimento, como caracteres repetidos ou pontuação; uma consulta, quando indicada, mantém as que a contêm. **Indicadores** mantém as cadeias que parecem um URL, um endereço IP, um endereço de e-mail, um caminho de ficheiro, uma linha de comandos, um segredo (uma chave privada, um token de acesso ou uma atribuição de palavra-passe) ou uma sequência longa de base64, e identifica cada uma.

O Borg também lê as cadeias dos ficheiros que o processo mapeou, até 1 GiB, e marca com • as que só a memória contém: texto que o processo construiu, recebeu ou descodificou em vez de o carregar dos seus ficheiros. Os indicadores vêm primeiro, as cadeias que só a memória contém vêm antes das que os seus ficheiros contêm, e as cadeias que se leem como palavras vêm antes das restantes. A lista mantém as 2000 melhores. Cada linha mostra o tipo e o número de cópias onde as outras correspondências mostram a sua região, e a pré-visualização mostra a cadeia no seu lugar; **C** percorre as suas cópias. Estes resultados são pistas para uma análise mais atenta, não prova de nada: os programas comuns contêm muitos URL, caminhos e chaves.

**I**, no mapa de memória ou entre as correspondências da pesquisa, verifica se o processo executa o código que os seus ficheiros contêm. Compara o código em memória com os ficheiros a partir dos quais foi mapeado e indica as páginas que diferem, procura na memória que nenhum ficheiro suporta os cabeçalhos de imagens executáveis (ELF, Mach-O e PE), e lista a memória executável que nenhum ficheiro suporta, a memória simultaneamente gravável e executável, e o código de ficheiros eliminados, sem nome ou temporários. Verifica também o processo no seu todo: bibliotecas injetadas através de `LD_PRELOAD`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES` ou `/etc/ld.so.preload`, caminhos de pesquisa de bibliotecas, um rastreador no Linux, um programa cujo ficheiro desapareceu ou foi substituído, e um nome diferente do do seu programa.

As descobertas ocupam o painel da lista, primeiro os alertas e depois os avisos e as notas. Selecionar uma pré-visualiza os bytes a que diz respeito, com a evidência realçada, e as linhas abaixo da lista dizem o que mais a poderia explicar. Os programas que compilam código enquanto correm, como os navegadores, o Java e os motores de JavaScript, contêm constantemente código que nenhum ficheiro suporta, por isso para eles essas descobertas são notas. **R** verifica de novo, **Ctrl-C** interrompe uma verificação e mantém o que encontrou, e **Esc** volta atrás. Uma verificação lê no máximo 512 MiB de código e 1 GiB de outra memória, durante no máximo 45 segundos, e indica quando parou mais cedo.

No Linux, as tabelas de páginas indicam que páginas de código um processo alterou, por isso só essas são lidas; no macOS também não é lida uma região sem páginas modificadas. O código na cache partilhada do macOS não tem ficheiro próprio e não é comparado. A memória que uma verificação não consegue ler é contabilizada e oferece **A: Administrador**. As descobertas são pistas, não veredictos: depuradores, perfiladores, atualizadores e ferramentas de segurança deixam os mesmos vestígios.

As correspondências ocupam o painel de lista e mostram o seu endereço, codificação e região originais. A seleção de uma correspondência visualiza os bytes circundantes no painel oposto, com os bytes correspondentes destacados. Enter foca a visualização; Esc volta o foco para as partidas. Esc das partidas restaura o mapa anterior ou lista de processos. A pesquisa de um instantâneo armazenado utiliza os seus bytes capturados sem ler o processo em execução. As pesquisas de processos são lidas por partes, reportam bytes inacessíveis e param após 2000 correspondências ou 60 segundos. **N** continua a partir dessa posição; **Ctrl-C** pára preservando os resultados. Os relatórios de cobertura distinguem os bytes pesquisados ​​e inacessíveis. Um processo pode alterar a sua memória enquanto está a ser pesquisado.

Leituras restritas de mapas de memória ou instantâneos e pesquisas parcialmente inacessíveis oferecem **A: Administrador**. Em macOS abre o prompt de autorização do sistema; em Linux ele usa `pkexec` e necessita de um agente de autorização disponível. Apenas um auxiliar de inspeção de curta duração é elevado. Borg continua a funcionar normalmente e o auxiliar apenas devolve dados do mapa ou bytes limitados. O macOS impede que qualquer outro programa, mesmo como administrador, leia a memória dos seus próprios programas (Proteção da Integridade do Sistema) e dos programas compilados com o Hardened Runtime. O Borg lê isto da assinatura do programa: os detalhes do processo indicam **Memória protegida**, uma leitura recusada explica porquê e **A: Administrador** não é oferecido, pois não ajudaria. Os seus mapas de memória e ficheiros abertos continuam visíveis. Também não é oferecido quando o Borg já é executado como administrador. A política de segurança do Linux pode ainda negar o acesso; Borg mostra a falha e deixa o monitor de processo comum utilizável.

A identidade do processo inclui a sua hora de início, pelo que um PID reutilizado não herda o histórico de outro processo. Os mapas de memória, os resultados de inspeção, os instantâneos e o histórico são limitados. Borg Commander permanece visível no canto inferior direito, incluindo a consola, o visualizador, o monitor, as caixas de diálogo e o protetor de ecrã.
