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

No Linux, a compilação inclui também os programas eBPF do observador de processos, que precisam
de um compilador nightly e do `bpf-linker`. Quando faltam, a compilação para e mostra os dois
comandos que os instalam. Para compilar sem os programas, defina `BORG_NO_EBPF=1`. O observador
ouve então os eventos de processo do kernel:

```sh
BORG_NO_EBPF=1 cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

O suporte para Homebrew já está preparado e passará a funcionar quando o tap for publicado:

```sh
brew install 0xcd21h/tap/borg-commander
```

Execute `borg` na pasta que pretende explorar. F10 sai e Ctrl-O passa da consola para os painéis.
Para atualizar, execute `borg update` numa cópia transferida, ou use o Homebrew ou o cargo se
tiver instalado dessa forma. O Borg só instala uma atualização quando a versão publicada traz a
assinatura do projeto. Se a assinatura faltar ou estiver errada, a cópia instalada fica como está.
`borg --version` mostra a versão instalada. As sessões abertas
passam para a nova versão e mantêm as suas shells (consulte
[Atualizar com sessões abertas](#updating-with-sessions-open)).

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
Enter avance para a seguinte, e Ctrl-Q mostra-o aí na pré-visualização, com a pesquisa marcada. Os
ficheiros Markdown e JSON abrem como código-fonte em ambos, porque a pesquisa leu o ficheiro tal
como está, e `s` muda para a vista formatada. F4 abre o ficheiro no editor do Borg nessa
correspondência, com a correspondência selecionada e a barra de pesquisa aberta na pesquisa, e Alt-E
faz o mesmo no outro painel, para que os resultados continuem à vista. O Borg lê os ficheiros em
segundo plano, e a linha de contexto conta os ficheiros lidos. Os ficheiros binários (um byte NUL
nos primeiros 8 KiB) e os ficheiros com mais de 64 MiB ficam por ler e contam como binários ou
demasiado grandes. Ctrl-C interrompe a pesquisa, que para nos 20 000 ficheiros como qualquer
pesquisa.

### Analisar ficheiros com regras

Defina **Procurar** como **Correspondências de regras** na caixa de diálogo de Ctrl-F para analisar
os ficheiros abaixo da pasta com regras YARA: as que o Borg inclui e as suas. O padrão de nome, o
**Tamanho mínimo** e **Omitir resultados de compilação, caches e ficheiros ignorados** continuam a
aplicar-se. São as regras que decidem o que procurar, por isso a caixa de diálogo deixa de mostrar
**Com o texto** e **Mínimo de ficheiros**.

Cada resultado mostra na coluna **Correspondência** a regra mais grave a que o ficheiro corresponde,
a quantas outras corresponde e os primeiros bytes que essa regra encontrou, como texto entre aspas
ou em hexadecimal. F3 abre o ficheiro no visualizador nesses bytes, com a barra de pesquisa
preenchida com eles, e F4 e a pré-visualização também abrem nesse ponto. A linha de contexto conta
os ficheiros analisados e as regras usadas. Os ficheiros com mais de 64 MiB ficam por ler e contam
como demasiado grandes, e a análise para nos 20 000 ficheiros como qualquer pesquisa. Ctrl-C
interrompe-a.

As regras do próprio Borg procuram:

- `reverse_shell`: uma shell interativa através de `/dev/tcp`, `nc` ou `socat` a executar uma shell,
  ou uma shell lançada num pseudoterminal.
- `download_and_run`: um script transferido e encaminhado diretamente para uma shell.
- `decode_and_run`: base64 descodificado e executado como comando da shell ou como código Python.
- `crypto_miner`: o endereço de um pool de mineração, ou as definições de um minerador comum.
- `upx_packed`: um executável comprimido com UPX, que se descomprime a si próprio em memória.
- `execve_shellcode`: o caminho da shell que o código de exploração constrói antes de iniciar uma
  shell.
- `ssh_key_planted`: uma chave pública acrescentada a um ficheiro `authorized_keys`.
- `reboot_job_fetch`: uma tarefa cron que corre em cada arranque e transfere, descodifica ou executa
  algo a partir de uma pasta temporária.
- `password_prompt`: uma caixa de diálogo AppleScript que pede uma palavra-passe com a resposta
  oculta, como fazem os programas que roubam dados no macOS.
- `launch_agent_from_temp`: um agente ou daemon de arranque do macOS que inicia um programa a partir
  de uma pasta temporária ou partilhada.
- `preload_write`: uma escrita em `/etc/ld.so.preload`, que carrega uma biblioteca em todos os
  programas no Linux.
- `history_wiping`: o histórico da shell desativado, ou registos do sistema esvaziados.
- `agent_unattended`: um agente de programação com IA iniciado com as aprovações desligadas, como
  fez o ataque à cadeia de fornecimento do Nx para procurar segredos nas máquinas.
- `hidden_prompt`: uma sequência de caracteres de etiqueta Unicode invisíveis, que podem esconder
  instruções para modelos de IA em prompts, ficheiros de regras e páginas web.
- `pickle_runs_code`: um pickle, o formato em que o Python e o PyTorch guardam modelos, que executa
  um comando quando é carregado.

Coloque as suas regras na pasta `rules` ao lado de `config.toml`: `~/.config/borg-commander/rules`,
ou a mesma pasta em `$XDG_CONFIG_HOME` ou `$BORG_CONFIG_DIR`. O Borg lê todos os ficheiros `.yar` e
`.yara` que aí estão e nas pastas abaixo, cada um num espaço de nomes próprio, por isso dois
ficheiros podem usar os mesmos nomes de regras. Cada ficheiro é compilado separadamente. Um ficheiro
com um erro fica de fora por inteiro, a linha de estado indica-o com a mensagem do compilador, a
linha de contexto conta os ficheiros deixados de fora, e os outros ficheiros continuam a correr.
Como cada ficheiro é carregado separadamente, um ficheiro que use `include` também fica de fora, e
os ficheiros que inclui correm por si.

As regras podem importar os módulos `hash`, `math`, `string`, `time`, `elf`, `macho`, `pe`, `dotnet`
e `dex`; um ficheiro que importe `magic`, `cuckoo` ou `console` fica de fora. As variáveis
`filename`, `filepath` e `extension` contêm o nome, o caminho e a extensão do ficheiro analisado,
como esperam os conjuntos de regras escritos para o THOR e o LOKI, e `filetype` e `owner` ficam
vazias. A meta `severity` de uma regra define a importância de uma correspondência: `alert`,
`critical` ou `high` dão um alerta, `warning`, `medium` ou `suspicious` um aviso, e `notice`, `low`
ou `info` uma nota. Sem ela, um `score` de 80 ou mais dá um alerta, de 60 ou mais um aviso, e abaixo
disso uma nota. Uma regra sem nenhuma das duas conta como aviso. Os seus campos `description` e
`false_positives` aparecem com cada correspondência.

O Borg não inclui conjuntos de regras públicos, porque as licenças variam. Transfira os que
considera fiáveis para a pasta de regras e verifique cada licença antes de os usar no trabalho:

- [YARA Forge](https://yarahq.github.io/) reúne em pacotes testados regras de muitas fontes
  públicas; cada regra mantém a licença da sua fonte.
- [signature-base](https://github.com/Neo23x0/signature-base) é gratuito para uso não comercial (CC
  BY-NC 4.0).
- [Os protection artifacts da Elastic](https://github.com/elastic/protections-artifacts) estão sob a
  Elastic License 2.0.
- [As regras da ReversingLabs](https://github.com/reversinglabs/reversinglabs-yara-rules) estão sob
  a licença MIT.

As regras correspondem a padrões de bytes e nada sabem de intenções. Instaladores, ferramentas de
segurança e os seus dados de teste também correspondem, e um ficheiro a que nenhuma regra
corresponde pode mesmo assim ser malicioso. Encare as correspondências como pistas. [Analisar a
memória com regras](#scanning-memory-with-rules) explica o que uma análise não consegue ver.

### Encontrar executáveis compactados

Defina **Procurar** como **Executáveis compactados** na caixa de diálogo de Ctrl-F para listar os
programas abaixo da pasta cujo código é tão denso como bytes comprimidos ou cifrados. Os
compactadores e os protetores deixam código assim, tal como o malware que esconde o seu código até
ser executado. O Borg lê as partes de cada ficheiro ELF, Mach-O e PE que o carregador mapeia como
executáveis, incluindo cada fatia de um binário Mach-O universal, e mede a entropia de até 1024
blocos de 4 KiB distribuídos uniformemente por elas. Um programa é listado quando pelo menos 70%
desses blocos chegam a 7,2 bits por byte. O código compilado da forma habitual fica muito abaixo
disso. O padrão de nome, o **Tamanho mínimo** e **Omitir resultados de compilação, caches e
ficheiros ignorados** continuam a aplicar-se, e a caixa de diálogo deixa de mostrar **Com o texto**
e **Mínimo de ficheiros**.

A coluna **Correspondência** indica a percentagem de blocos com entropia alta e a entropia mais alta
encontrada, como "97% alta · máx. 8". F3 abre o programa nas linhas hexadecimais do visualizador, no
primeiro bloco com entropia alta, com a faixa de entropia descrita em [Visualizador e
pré-visualização](#viewer-and-quick-view) ao lado. A linha de contexto conta os programas lidos e os
outros ficheiros, onde o Borg não sabe encontrar código. Ctrl-C interrompe a pesquisa, que para nos
20 000 ficheiros como qualquer pesquisa. Um programa compactado só mostra o seu verdadeiro código
enquanto corre, e a [Análise de entropia de todos os processos](#entropy-scan-of-every-process)
examina os programas dessa forma.

### Trabalhar com os resultados

Clique no cabeçalho de uma coluna para ordenar por ela e clique de novo para inverter a ordem. Os
menus Esquerda e Direita também permitem ordenar, e o menu Pesquisar acrescenta a ordenação por
pasta de origem e o agrupamento por pasta ou tipo de ficheiro. Enter no cabeçalho de um grupo
recolhe-o; Insert marca os ficheiros correspondentes desse grupo. As marcas mantêm-se quando
ordena ou filtra, e o rodapé conta também os ficheiros marcados que o filtro oculta.

F3 e Ctrl-Q abrem o ficheiro original e, depois de uma pesquisa de texto, abrem-no na primeira
correspondência. A pré-visualização acompanha o cursor pelos resultados e mostra cada ficheiro na
sua correspondência, com a pesquisa marcada como a barra de pesquisa a marca. Alt-E abre o ficheiro
no editor do Borg no outro painel, para que os resultados continuem à vista. Esc ou F10 fecha esse
editor e devolve o foco aos resultados, e a pré-visualização regressa se estava aberta antes. F5
copia para uma pasta real; se o outro painel for uma coleção, F5 acrescenta-lhe referências. F6
precisa de uma pasta real como destino. Numa coleção, F8 remove referências e não mexe nos
ficheiros; nos resultados de pesquisa, F8 pede confirmação para eliminar os ficheiros originais.
Shift-F8 também elimina os ficheiros originais a partir de uma coleção, depois de confirmar. Quando
estão marcados uma pasta e ficheiros dentro dela, a tarefa trata a pasta uma única vez. **Revelar
original** abre a pasta do ficheiro no outro painel. Left ou Esc ocultam os resultados, e
Alt-Shift-R repõe-nos onde os deixou. Tab alterna entre os painéis mesmo quando a shell tem um
comando por terminar. A shell fica sempre numa pasta real. F10 pede sempre confirmação antes de
sair.

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
| Ctrl-PgDn / Ctrl-PgUp | Abrir a pasta ou o arquivo sob o cursor / ir para a pasta pai |
| Tab | Alternar entre painéis, ou entrar na pré-visualização e voltar à lista de ficheiros |
| Ctrl-O | Mostrar a shell / voltar aos painéis |
| Alt-I | Contar tamanhos de pastas em segundo plano / parar a contagem |
| Ctrl-S | Localização rápida na pasta |
| Ctrl-\ | Pastas favoritas: saltar para uma pasta fixada ou recente |
| Ctrl-Q | Pré-visualização: mostra o ficheiro sob o cursor (texto, imagem ou pasta) no outro painel |
| Ctrl-L | Informações: mostra tamanho, permissões, proprietário, datas, espaço livre e o disco no outro painel |
| Insert, Ctrl-T, Shift-Up/Down | Marcar ficheiros |
| `+` `-` `*` | Selecionar, desselecionar ou inverter por padrão (`*.rs`, `build/`) |
| Alt-Enter | Colocar os nomes marcados (ou o atual) na linha de comando; em `..`, o caminho da pasta pai |
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
| Alt-O | Abrir a pasta deste painel no outro painel |
| Alt-J | Tarefas: pausar ou cancelar cópias, movimentações e eliminações em curso |
| Alt-M | Monitor de atividade: processos e memória |
| Alt-C | Docker: projetos Compose e contentores |
| Alt-N | Rede: portas, ligações e tráfego |
| Alt-U | Itens de arranque: o que arranca sozinho, alteração mais recente primeiro |
| Alt-L | Alertas: o que o observador de processos encontrou |
| Alt-T | Time Machine: o que as cópias de segurança deixam de fora, no macOS |
| Alt-E | Editar o ficheiro selecionado neste painel, em vez dos seus ficheiros, ou no outro painel ao lado dos resultados |
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

Quando volta à pasta pai, o cursor fica na pasta de onde veio, na mesma linha do ecrã e com a
lista na mesma posição em que estava quando entrou. Se entretanto surgiram ou desapareceram
ficheiros, ou se o painel mudou de altura, o cursor fica na mesma pasta, tão perto da linha anterior
quanto a lista permitir.

Alt-Enter escreve os nomes tal como a shell os vê a partir da pasta do painel. Em `..` escreve o
caminho completo da pasta pai, porque o nome da pasta pai, sozinho, apontaria para dentro da pasta
atual. Nos resultados de pesquisa e nas coleções escreve caminhos completos, já que os ficheiros vêm
de pastas diferentes.

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

Os atalhos também funcionam com esquemas de teclado não latinos: russo, ucraniano, bielorrusso,
cazaque, grego, hebraico, árabe, georgiano, coreano e tailandês. Uma letra de um destes esquemas
premida com Ctrl ou Alt conta como a tecla que ocupa o mesmo lugar num teclado americano, tal como
uma letra premida num ecrã ou numa caixa de diálogo que não está à espera de texto. Com o esquema
russo ativo, Alt-M abre o monitor de atividade e Ctrl-C chega à shell. A linha de comandos, o
editor e os campos do Borg mantêm as letras que escreve. As perguntas com os botões **[ Sim ]** e
**[ Não ]** aceitam Y e N em qualquer idioma e também a primeira letra de cada botão no idioma do
Borg, como S para Sim e N para Não em português.

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

### Saltar para pastas

Escreva `cd ` na linha de comandos e o Borg mostra, logo acima dela, as pastas para onde pode
saltar: primeiro as favoritas, marcadas com ★, depois as pastas que os painéis abriram, das mais
recentes para as mais antigas. Cada palavra que acrescenta restringe a lista e pode corresponder a
qualquer parte do caminho tal como a lista o mostra, por isso `cd pro bo` encontra
`~/Projects/borg`. A pasta pessoal aparece como `~`, por isso uma palavra do caminho dela, como `me`
em `/home/me`, não corresponde a nenhuma pasta lá dentro. Uma palavra que comece por `/` corresponde
ao caminho completo. As pastas cujo nome corresponde à última palavra ficam em primeiro lugar, junto
à linha de comandos. Prima Up para entrar na lista. Enquanto a lista está visível, Up e Down só se
movem nela e param nas duas pontas, por isso o cursor do painel mantém a posição. Enter abre a pasta
escolhida no painel ativo. Tab coloca-a na linha de comandos a seguir a `cd ` sem executar o
comando, com escapes nos espaços e noutros caracteres que a shell dividiria ou expandiria, e a lista
passa para as pastas que ela contém. Um clique numa pasta também a abre. Para executar o seu próprio
`cd`, continue a escrever e prima Enter.

Escreva um caminho e a lista começa pelas pastas que ele contém. `cd /Volumes` mostra os volumes
montados, e `cd ~/Projects/bo` restringe a lista às pastas de `~/Projects` com `bo` no nome,
primeiro as que começam por essas letras. Qualquer outro comando mostra a mesma lista para um
caminho com `/`, como `cp notes.txt ~/Documents/`. Aí, Tab ou Enter escreve o nome da pasta
escolhida no caminho, com escapes ou dentro das suas aspas, como a shell precisa, e a lista passa
para as pastas que ela contém. Esc oculta a lista e devolve Up e Down ao painel. A lista volta
quando começa outra palavra ou apaga a que está a escrever, e Esc logo a seguir a um espaço oculta-a
apenas até voltar a escrever. As pastas ocultas aparecem quando escreve um ponto, ou quando os
painéis mostram ficheiros ocultos. O Borg lê até 10 000 subpastas de uma pasta, e só na linha de
comandos desta máquina; a consola de um contentor ou de uma máquina remota mantém a conclusão da
própria shell.

Uma linha de comandos que apenas muda de pasta é executada sem que os painéis saiam do ecrã, e o
painel ativo acompanha a shell até à nova pasta. Quando a pasta não existe, um aviso indica-o e
Ctrl-O mostra a mensagem da shell. Uma pasta que demora a abrir, como uma numa partilha de rede
lenta, faz aparecer a consola até o comando terminar. Tudo o que vá além de um `cd`, como
`cd src && make`, mostra a consola como habitualmente.

Ctrl-\ ou **Comando → Pastas favoritas** abre as favoritas com a pasta do painel ativo escolhida.
Escreva para restringir a lista, escolha com Up e Down e prima Enter para abrir a pasta. Insert
fixa a pasta escolhida nas favoritas, por isso Ctrl-\ seguido de Insert fixa a pasta onde está.
F8 ou Delete retira uma pasta das favoritas ou das pastas recentes. Esc limpa o que escreveu e um
segundo Esc fecha a janela. As favoritas guardam até 256 pastas.

Alt-O ou **Comando → Mesma pasta no outro painel** abre a pasta do painel ativo no outro painel,
para que ambos mostrem o mesmo sítio. A pasta ocupa o lugar da pré-visualização, das Informações ou
dos ficheiros de um contentor ou de uma máquina remota nesse painel. Se o outro painel mostrava
resultados de pesquisa ou uma coleção, Alt-Shift-R nesse painel volta a mostrá-los. A partir de
resultados de pesquisa, Alt-O abre a pasta onde a pesquisa começou. Uma coleção não tem pasta
própria, e as pastas de contentores e as remotas só abrem num painel, por isso nesses casos Alt-O
mostra uma notificação, tal como acontece quando o outro painel tem um editor. Enquanto escreve um
comando, Alt-O passa para a shell.

As favoritas ficam guardadas em `config.toml`, na secção `[folders]`. O Borg lembra-se das últimas
64 pastas que os painéis abriram, na sua pasta de estado, num ficheiro que só o utilizador pode
ler. Defina `history = false` em `[folders]` para deixar de as guardar.

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

A CPU aparece duas vezes. **Agora %** é a leitura mais recente, e **Média %** é a média dos
últimos segundos, como no monitor de atividade, por isso mantém-se estável quando um contentor
trabalha aos repelões. Uma lista demasiado estreita para os dois mostra apenas **Média %**. Os
detalhes mostram o mesmo par como **CPU agora** e **CPU média**, e um projeto soma ambos para os
seus contentores em execução.

| Tecla | Ação |
| --- | --- |
| ↑ ↓, PgUp, PgDn, Home, End | Mover |
| ← → / Enter | Recolher ou expandir um projeto; Enter num contentor passa para os detalhes |
| S / X / R | Iniciar / parar / reiniciar o contentor, ou os contentores do projeto |
| P | Pausar ou retomar |
| F | Explorar os ficheiros do contentor |
| E, Alt-2 | Abrir uma shell no contentor, ou voltar à shell já aberta |
| Alt-1 | Voltar a mostrar a visão geral do contentor, também a partir da shell |
| D | Mostrar o espaço em disco, ou voltar aos contentores |
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

### Espaço em disco e limpeza

**D** na vista Docker, ou **F9 → Docker → Espaço em disco**, mostra o que o Docker guarda em disco
no lugar da lista de contentores, com uma linha por tipo: **Cache de compilação**,
**Imagens sem nome**, **Imagens não usadas**, **Imagens em uso**, **Contentores parados**,
**Volumes anónimos não usados**, **Volumes com nome não usados**, **Volumes em uso** e
**Redes não usadas**. Cada linha tem uma contagem, um tamanho e o espaço que a remoção liberta, e
a linha do topo soma tudo o que pode recuperar. O Borg mede com `docker system df --verbose` em
segundo plano, o que pode demorar num motor muito ocupado. **R** volta a medir, e o Borg faz o
mesmo quando o comando seguinte no prompt termina depois de **F8** ter escrito um.

As imagens sem nome são compilações a que já nada dá nome, normalmente porque a etiqueta passou
para uma compilação mais recente. As imagens não usadas têm nome, e nenhum contentor foi criado a
partir delas. As imagens e os volumes que os contentores usam, em execução ou parados, ficam até
esses contentores serem removidos, pelo que as suas linhas não libertam nada. Uma imagem só
liberta as camadas que nenhuma outra imagem partilha, e por isso o tamanho recuperável pode ficar
muito abaixo do seu tamanho.

→ ou Enter expande um tipo nos seus itens, do maior para o menor, e ← volta a recolhê-lo. O outro
painel mostra o item selecionado: as etiquetas de uma imagem, o ID, o tamanho partilhado, a data
de criação e o número de contentores; o tipo de um registo da cache de compilação, se está
partilhado ou em uso e quando uma compilação o usou pela última vez; a imagem e o estado de um
contentor; o tamanho de um volume e os contentores que o montam. Os tamanhos que o Docker não
conseguiu medir aparecem como `?`, e `/` filtra todos os tipos de uma vez.

**F8** escreve o comando de limpeza da linha selecionada no prompt da shell do host e mostra a
consola. Nada é executado até premir Enter, por isso pode ler o comando, alterá-lo ou limpá-lo
antes com Ctrl-U. Num tipo, F8 escreve o respetivo prune, como `docker image prune -a` para as
imagens não usadas ou `docker volume prune` para os volumes anónimos não usados, e o Docker pede
confirmação antes de eliminar o que quer que seja. Num item, F8 escreve `docker rmi`, `docker rm`
ou `docker volume rm` com esse item, ou com todas as imagens, contentores ou volumes marcados.
Space ou Insert marca um item, e `*` inverte as marcas do tipo selecionado. Uma imagem sai por
todas as etiquetas, já que o Docker a elimina com a última, ou pelo ID quando não tem nenhuma. Os
registos da cache de compilação saem todos juntos com `docker builder prune`. O Borg nunca
acrescenta `-f` nem `--force`. Um nome com um carácter de controlo não recebe nenhum comando,
porque o prompt tomaria esse carácter por uma tecla.

Os volumes guardam os dados que os contentores gravaram, como os ficheiros de uma base de dados, e
um volume removido não pode ser recuperado. O outro painel repete este aviso junto a cada comando
de volumes. Quando a CLI `docker` tem mais de um contexto, os comandos indicam aquele com que o
Borg mediu, como em `docker --context staging image prune`, para chegarem ao mesmo motor seja qual
for o contexto que a shell escolheria. Vão sempre para a shell do host, também enquanto um painel
mostra os ficheiros de um contentor, porque a vista Docker mantém a linha de comando no host.

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
contentores, e mostrar um ficheiro ou uma pasta no seu outro painel. Também pode listar as portas
abertas, as ligações e as interfaces de rede com o respetivo tráfego, medido durante um segundo e
filtrado com as mesmas palavras que o filtro do ecrã de rede (consulte [Filtrar](#filtering)).
Também pode listar os itens de arranque, com a alteração mais recente primeiro, com os seus
programas e as regras a que corresponderam, filtrados com as mesmas palavras que o filtro desse ecrã
(consulte [Itens de arranque](#startup-items)).

Tudo o que altera a máquina fica à sua espera numa caixa de diálogo que mostra a alteração
completa: um comando e o sítio onde vai ser executado, um ficheiro novo ou uma edição sob a forma
de diff. Percorra as alterações longas com as setas, PgUp e PgDn ou a roda do rato, e prima F3
para ver a caixa de diálogo em ecrã inteiro. A opção Não mantém-se selecionada até escolher Sim,
e a caixa de diálogo ignora o Sim durante o primeiro meio segundo, para que uma tecla ou um clique
destinados a outra coisa não a possam aprovar. Uma edição é recusada se o ficheiro tiver mudado
enquanto decidia. Um comando com tabulações, sequências de escape
ou outros caracteres de controlo também é recusado, porque a caixa de diálogo não os consegue mostrar.
A caixa de diálogo aparece por cima de tudo, incluindo o editor, o visualizador, outra caixa de
diálogo e o protetor de ecrã, e recebe todas as teclas até responder. O que ela tapava volta a
aparecer depois. Se o agente deixar de esperar antes disso, a caixa de diálogo fecha-se e um aviso
indica-o.
Para um computador emparelhado, `approval_mode` pede-lhe confirmação uma vez antes de ativar
Aprovar tudo nessa sessão. Enquanto estiver ativo, fica visível um distintivo; clique nele ou prima
Ctrl+Alt+A para o desativar de imediato. Desativa-se também quando essa ligação emparelhada fecha.
As aprovações de emparelhamento são independentes.

Um pedido chega também ao seu próprio computador quando um painel remoto aí mostra a sessão em
que o pedido espera (consulte [Painéis remotos](#remote-panels)). O seu borg mostra-o no mesmo
diálogo, com o nome do servidor no título, por cima do que estiver a fazer, e envia a sua resposta
de volta pela ligação cifrada. O servidor mantém também o seu próprio diálogo aberto. Conta a
primeira resposta, venha de onde vier, e os outros diálogos fecham com um aviso que diz onde foi
respondido. Um pedido que o agente retire fecha em todo o lado. Quando outro computador controla a
sessão através de um espelho, o seu diálogo mostra o pedido sem Sim e Não, e Esc fecha-o apenas no
seu ecrã. Enquanto essa sessão aprovar tudo, o seu borg mostra um distintivo com o nome do
servidor; clique nele ou prima Ctrl+Alt+A para desativar Aprovar tudo lá de imediato. Só as suas
teclas e cliques respondem a estes diálogos: as ferramentas do agente, incluindo `type`, não
conseguem premir nada neles.

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
indica o processo, o programa e a pesquisa. Depois de aprovar, o agente pode voltar a pesquisar e
analisar esse processo até o borg terminar. Pode pedir para ler como administrador, o que abre o
pedido de palavra-passe do próprio sistema.

O agente também pode analisar com regras YARA e verificar a integridade do código, como fazem
**Correspondências de regras** na caixa de diálogo de Ctrl-F e as teclas **Y** e **I** no mapa de
memória (consulte [Analisar ficheiros com regras](#scanning-files-with-rules), [Verificação de
integridade do código](#code-integrity-check) e [Analisar a memória com
regras](#scanning-memory-with-rules)). Um ficheiro ou uma pasta na máquina onde o borg corre é
analisado sem perguntar, tal como uma leitura. A análise de uma pasta lê todos os ficheiros abaixo
dela no mesmo sistema de ficheiros, incluindo os ocultos. Deixa por ler os ficheiros com mais de 64
MiB e para ao fim de 20 000 ficheiros, 2000 correspondências ou 45 segundos. Os ficheiros em
contentores não são analisados.

Com um processo é diferente, porque a análise lê a sua memória. A primeira análise de cada processo
aguarda a sua aprovação em **Pedido remoto para analisar a memória de um processo**, que indica o
processo, o programa e as verificações: a verificação de integridade, as regras, ou ambas. Uma
aprovação cobre as análises e as pesquisas de memória desse processo até o borg terminar, seja qual
for a que o agente pediu primeiro. Cada verificação para ao fim de 45 segundos, 1 GiB de memória ou
2000 resultados. O agente pode pedir para ler como administrador, o que abre o pedido de
palavra-passe do próprio sistema uma única vez para as duas verificações.

As regras são as do Borg e as da sua pasta de regras, a não ser que o agente traga o seu próprio
texto de regras, até 64 KiB. Assim, pode redigir uma regra a partir do que encontrou, experimentá-la
numa pasta ou num processo e afiná-la. Esse texto corre sozinho, sem as regras do Borg nem as suas,
não pode usar `include` e nunca é guardado. O texto que não compila volta para o agente com a
mensagem do compilador, antes de qualquer caixa de diálogo. Para um processo, a caixa de diálogo
mostra o texto de regras do agente abaixo das verificações, para que veja o que ele procura.

O agente recebe cada resultado, do mais grave para o menos grave, com a gravidade, o tipo, a regra e
a sua origem, o ficheiro e o deslocamento ou o endereço e a região de memória, até 64 bytes do que
correspondeu, como texto ou em hexadecimal, e o que mais o poderia explicar. Seguem-se quanto foi
lido, o que não foi possível ler, se a análise terminou e os ficheiros de regras deixados de fora.
As provas vão para o agente tal como foram encontradas, incluindo chaves e tokens, como na pesquisa
de memória.

O agente também pode capturar tráfego como a tecla **R** faz no ecrã de rede (consulte
[Capturar tráfego](#capturing-traffic)). Indica o que gravar: uma porta, uma ligação, um processo
pelo PID ou todo o tráfego da interface. Para um processo, o Borg grava a interface inteira e
resume as conversas desse processo. Uma captura dura de 1 a 60 segundos, ou 5 se o agente não
indicar a duração. Quando o agente não indica a interface, o Borg mede durante um segundo e escolhe
a que a caixa de diálogo **Capturar tráfego** escolheria. Cada captura aguarda a sua aprovação
numa caixa de diálogo, **Pedido remoto para capturar tráfego**, que mostra o tráfego, a interface,
a duração, quanto de cada pacote guardar e a pasta. Enquanto decorre, o rodapé mostra-a, e no fim
um aviso indica onde o Borg a guardou: na pasta do painel ativo ou numa pasta que o agente indique,
legível só por si. A captura de um agente é sempre executada com o seu utilizador e nunca pede a
palavra-passe de um administrador, por isso sem acesso termina com **Sem permissão para capturar
em** e o nome da interface.

O agente recebe um resumo: os protocolos que o Borg reconheceu e depois cada processo, host e
ligação com os seus pacotes e bytes, com os nomes dos hosts tirados das respostas DNS da própria
captura e dos nomes de servidor TLS. O Borg acompanha os sockets enquanto a captura decorre, por
isso as conversas ficam atribuídas aos seus processos mesmo com o ecrã de rede fechado. O resumo
deixa de fora o que os pacotes transportam. Ler um ficheiro `.pcap`, `.pcapng` ou `.cap` dá o mesmo
resumo, restringido com as palavras do filtro do visualizador de tráfego, e o agente pode pedir um
pacote pelo número para obter as camadas e os bytes num dump hexadecimal. O Borg lembra-se dos
processos das últimas 16 capturas do agente até terminar. Os outros ficheiros mostram as conversas
em **Processo desconhecido**, a menos que os pacotes tragam o seu processo.

Enquanto houver um agente ligado, **◈ Remoto** aparece no início da barra de atalhos. Clique nele,
ou escolha F9 → Remoto → **Registo remoto…** a qualquer momento, para ver um registo do que o agente
fez: o que viu, leu, analisou, mostrou e escreveu, e todos os comandos e alterações, aprovados ou
recusados. Cada sessão guarda o seu registo no disco, legível apenas por si, para que possa voltar a
uma sessão que deixou a correr e ver o que aconteceu nela, mesmo depois de o Borg terminar. Quando o
registo fica grande, o Borg descarta as entradas mais antigas. Antes de guardar uma entrada, o Borg
oculta as palavras-passe, os tokens e as chaves que reconhece, e deixa de fora a saída dos comandos.
Uma palavra-passe passada como uma palavra simples parece uma palavra qualquer, por isso o Borg não
a consegue ocultar. Para não guardar registo nenhum, desative
**Remoto: manter um registo do que faz** em Opções → Plugins, o que também elimina o registo
guardado.

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
   A pergunta aparece de imediato, mesmo enquanto F3 mostra o comando em texto simples.
   Escolha Sim se os códigos coincidirem. Em seguida, `borg pair` propõe acrescentar o servidor ao
   Claude Code, como `claude mcp add borg-NAME -- borg mcp remote NAME`. Espera 5 minutos pela sua
   resposta; quando desiste ou o interrompe, a pergunta fecha-se e a ligação deixa de funcionar.

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
`borg get NAME:PATH [TO]` e `borg put FROM NAME:PATH` fazem o mesmo a partir do terminal. As
transferências do Claude vão para a pasta Transferências e nunca substituem um ficheiro que já lá
esteja. Os envios aguardam a sua aprovação no borg, que mostra o ficheiro enviado, e cada aprovação
indica o computador que fez o pedido.

A ligação é cifrada de ponta a ponta. Cada borg guarda uma chave de servidor, pela qual os
computadores emparelhados o conhecem, e um computador que chega a servidores guarda também uma chave
de computador própria. Só o utilizador pode ler ambas. A chave de servidor é um ficheiro na pasta
`link` da pasta de estado. A chave de computador também é um ficheiro aí. Num Mac fica no
porta-chaves de início de sessão, e no Linux o ficheiro é selado com uma palavra-passe (consulte
[Chaves de computador](#computer-keys)). O emparelhamento troca as chaves públicas, e cada ligação
abre com um handshake Noise IK associado a elas, pelo que o relay só reencaminha texto cifrado e não
consegue responder em nome de nenhum dos borgs. Os pedidos vindos de outro computador podem ler sem
pedir autorização, tal como os locais; ative "perguntar antes de outro computador ler" em Opções →
Plugins para que também essas leituras aguardem aprovação. O Borg não mantém nenhuma ligação ao
relay a menos que esteja emparelhado ou a mostrar uma ligação de emparelhamento, e não mantém
nenhuma quando o borg cloud está desativado em Opções → Plugins.

O Borg guarda o relay na configuração como `relay = "wss://relay.example.com"`. Um computador chega
ao servidor através do relay pelo qual emparelhou, por isso, depois de o servidor mudar de relay,
volte a emparelhar o computador. Todos os borgs de uma máquina partilham as chaves, e o
[serviço em segundo plano](#named-sessions-and-the-background-service) liga-os todos ao relay. Com o
serviço desativado, só o primeiro borg a ser aberto se liga, e os outros indicam qual deles tem o
borg cloud.

#### Chaves de computador

A chave de computador prova a cada servidor emparelhado que uma ligação vem do seu computador.
Quando um servidor ainda conhece este computador por outra chave, como a chave de servidor desta
máquina, o Borg muda-o para a chave de computador na próxima vez que abrir uma sessão lá. O Borg
prova ao servidor que tem a chave nova, e o servidor troca a chave na entrada desse computador e
mantém o nome. As ligações que ainda usam a chave antiga terminam no pedido seguinte. O servidor
mostra um aviso e escreve as duas impressões digitais no registo do agente, e a lista **Clientes**
mostra quando a chave mudou e qual era. Um servidor com um Borg mais antigo mantém a chave antiga
até ser atualizado.

Num Mac, o Borg guarda a chave de computador no porta-chaves de início de sessão, como um item
chamado **Borg Commander** que só o Borg pode ler sem perguntar. Na primeira vez que um processo do
Borg precisa da chave, o macOS pede o Touch ID ou a palavra-passe, por isso até um programa que
inicie o `borg` por conta própria fica à vista. Depois de cada atualização do Borg, o macOS pergunta
também uma vez se o novo Borg pode usar o item; escolha **Permitir sempre**. Um ficheiro de chave de
computador de um Borg anterior passa para o porta-chaves na primeira vez que o Borg o usa, e o
ficheiro é apagado. Um Borg executado sem o ecrã do Mac, como por SSH ou no serviço em segundo
plano, não pode mostrar o pedido nem aceder a servidores. Quando `$BORG_STATE_DIR` indica uma pasta
de estado diferente da habitual, o Borg guarda a chave de computador como ficheiro na pasta `link`
dessa pasta. As versões do Borg para macOS são assinadas com o Hardened Runtime, por isso outros
programas não podem ler a memória do Borg nem carregar código nele.

No Linux, o Borg sela a chave de computador com uma palavra-passe à sua escolha, no ficheiro
`computer-key.sealed` da pasta `link`. Execute `borg unlock` num terminal para a escolher: o Borg
sugere quatro palavras aleatórias, fáceis de escrever e difíceis de adivinhar, e pede a
palavra-passe duas vezes. O Borg reforça a palavra-passe com Argon2id, o que demora alguns segundos
e usa 1 GiB de memória, ou um oitavo da memória numa máquina com menos de 8 GiB, pelo que
adivinhá-la a partir de uma cópia do ficheiro é lento. Um ficheiro de chave de computador de um Borg
anterior é selado na primeira vez que executar `borg unlock`, e o ficheiro é apagado; até lá, o Borg
pede que o execute. Quando `$BORG_STATE_DIR` indica uma pasta de estado diferente da habitual, o
Borg também guarda no Linux a chave de computador como ficheiro normal, sem palavra-passe.

Depois de desbloqueada, a chave fica no keyring do kernel do utilizador até passarem 12 horas sem o
Borg a usar, e cada utilização volta a contar as 12 horas desde o início. Um reinício bloqueia-a, e
`borg lock` bloqueia-a de imediato. Onde o Borg não pode usar o keyring do kernel, como em muitos
contentores e serviços do sistema, um pequeno processo do Borg guarda a chave, num socket da pasta
`link` que só o utilizador pode abrir, e termina após 12 horas sem uso. Quando a chave está
bloqueada, o Borg pede a palavra-passe: os painéis abrem o diálogo **Desbloquear chave**, e
`borg pair`, `borg get`, `borg put` e `borg mirror` pedem-na no terminal. `borg mcp remote`, que o Claude inicia
sem terminal, indica que deve executar `borg unlock`.

`borg unlock --ssh-agent` permite também que o ssh-agent desbloqueie a chave. O Borg pede ao agente
que assine um desafio fixo com uma chave Ed25519, ou com uma chave RSA se não tiver nenhuma, e sela
a chave uma segunda vez com essa assinatura. A partir daí, o Borg desbloqueia a chave sem perguntar
enquanto esse agente estiver acessível, e a palavra-passe continua a funcionar. O Borg verifica que
o agente assina da mesma forma duas vezes e recusa uma chave cujas assinaturas mudem. Quem puder
usar o seu agente consegue obter a mesma assinatura: se o reencaminhar com `ssh -A`, o root da
máquina remota pode usá-lo enquanto estiver ligado e, com uma cópia do ficheiro selado, desbloquear
esta chave. Reencaminhe o agente apenas para máquinas de confiança, ou deixe isto desligado.
`borg unlock --no-ssh-agent` volta a desligá-lo.

`borg pair --new-key` dá a este computador uma chave nova, e cada servidor aceita-a na próxima vez
que o Borg se ligar a ele. Até todos a terem, o Borg guarda também a chave antiga, e `borg pair`
assinala cada servidor que ainda a aceita. Uma semana depois, sugere também esquecer os servidores
que já não usa, com `borg pair --forget NAME`.

Quando um servidor recusa todas as chaves que este computador tem, o Borg indica-o. Ou alguém
esqueceu este computador nesse servidor, ou alguém com uma cópia da chave mudou primeiro o servidor
para uma chave sua. Uma chave nova é higiene e não desfaz uma fuga. Se suspeitar que alguém copiou
uma chave, prima F8 em Acesso remoto em cada servidor para esquecer os computadores emparelhados,
emparelhe de novo e verifique esses servidores.

### Sessões com nome e serviço em segundo plano

Com espaços de trabalho com nome, `borg sessions list` mostra os respetivos IDs e nomes,
`borg sessions create NAME` acrescenta um e `borg --session NAME` abre-o. Através de MCP, o agente
pode chamar `list_sessions`, `list_remote_sessions` e `use_session` para escolher a sessão com que
essa ligação trabalha. Uma única chamada de ferramenta pode também passar um ID em `session` para
visar outra sessão apenas dessa vez. A sessão escolhida aplica-se apenas a essa ligação MCP.

Com mais de uma sessão, ou quando a única que existe está aberta noutro terminal, o `borg` começa
pela janela **Sessões do Borg**. As primeiras linhas explicam o que é uma sessão, e a lista mostra
cada sessão com a data em que foi usada pela última vez. Uma sessão ativa neste momento aparece
marcada com o seu estado: **Aberta noutro terminal**, ou **Mantida pelo serviço** quando o serviço
em segundo plano a executa. Fica escolhida a sessão usada por último, a menos que outro terminal a
tenha aberta. Enter abre a sessão escolhida onde a deixou; uma sessão que o serviço mantém abre-se
ligada a ele, como com `borg service attach NAME`. Um clique escolhe uma sessão e um segundo clique
abre-a. **F7** dá nome a uma nova sessão e abre-a, e Esc ou F10 sai sem abrir nenhuma. Enquanto a
janela está aberta, verifica a cada 2 segundos que sessões estão ativas.

**F8** ou **Delete** remove a sessão escolhida, depois de uma confirmação que tem **Não** por
omissão. Isto elimina os painéis guardados e o histórico de terminal da sessão; os seus ficheiros e
pastas mantêm-se. A sessão Default fica sempre, tal como uma sessão ativa neste momento: saia
primeiro do Borg no outro terminal. Para remover uma sessão que o serviço mantém, abra-a com
`borg service attach NAME` e saia com F10.

Quando indica com `borg --session NAME` uma sessão que outro Borg tem aberta num terminal, o
`borg` para e identifica esse Borg: o número do processo, o terminal tal como o `ps` o mostra e há
quanto tempo arrancou. Saia desse Borg com F10 no sítio onde está a correr. Se já não conseguir
chegar ao terminal dele, termine-o com `kill` e o número do processo.

Todos os seus Borgs numa máquina partilham um serviço em segundo plano. O primeiro `borg` que abrir
inicia-o, sem nada para instalar, e o serviço para 30 segundos depois de fechar a última sessão.
Para desativar isto, defina `start-service = false` na configuração.

O serviço mantém o borg cloud ligado para todos os seus Borgs, por isso um computador emparelhado
chega a todas as sessões abertas. Trabalha na sessão que indica, ou na que usou por último, e
aguarda aí a sua aprovação. Pode emparelhar a partir de qualquer Borg.

Para manter uma sessão em execução sem terminal, execute `borg service keep NAME`. Abra-a com
`borg service attach NAME` ou a partir da janela **Sessões do Borg**, e prima Ctrl-] para se
desligar. Sair do Borg aí com F10 termina a sessão. Quando um computador emparelhado pede uma sessão
que não está em execução, o serviço inicia-a em segundo plano.

Para iniciar o serviço com a máquina, execute `borg service install`, `enable` e `start`. Estes
comandos usam o sudo, que pede a sua palavra-passe, a menos que seja root. `borg service stop`,
`disable` e `uninstall` desfazem-nos. Também pode escolher F9 → Remoto →
**Serviço em segundo plano…**: a caixa de diálogo mostra o comando do passo seguinte, e Enter
executa-o na shell do Borg. Num contentor sem systemd, configure o supervisor de processos do
contentor para executar `borg service run`.

### Atualizar com sessões abertas

`borg update` instala a nova versão, passa o serviço em segundo plano para ela e avisa quando o
serviço já a corre. O serviço arranca de novo no seu próprio processo, por isso as sessões que
mantém conservam as suas shells e os programas que nelas correm. Cada Borg aberto acompanha-o
quando está inativo: espera que as tarefas em curso terminem, que os ficheiros editados sejam
guardados ou fechados e que o pedido do agente esteja concluído. Depois arranca de novo no mesmo
terminal, mantém a sua shell e indica a versão que corre agora.

A atualização guarda a versão que substituiu junto do ficheiro do programa, como `.borg.previous`.
Se a nova versão não assumir o lugar da anterior em 60 segundos, o Borg repõe a anterior e o
serviço volta a arrancar com ela. As sessões que mantinha arrancam então com shells novas.

Um serviço iniciado por um Borg mais antigo não consegue mudar sozinho de versão. O `borg update`
indica-o, e o serviço usa a nova versão da próxima vez que arrancar.

Para atualizar uma máquina emparelhada, selecione-a em [Hosts remotos](#remote-hosts) e prima
**U**. Depois de confirmar, o host obtém a versão mais recente tal como o `borg update` faz, e o
seu serviço passa para ela. **Esc** fecha o seletor enquanto a atualização continua. Em seguida, o
Borg indica a versão que o host corre, ou que a nova versão não arrancou e o host voltou à que
corria. Um espelho de uma das suas sessões espera por ele e volta a mostrar o host quando este
regressa (consulte [Espelhar uma sessão](#mirroring-a-session)). Execute antes `borg update` no
host quando o seu Borg for mais antigo ou correr sem o serviço em segundo plano. Um Borg instalado
aí com o Homebrew ou o cargo atualiza-se com essa ferramenta no host, e o Borg indica o comando.

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

Enquanto um painel remoto mostra uma sessão, os pedidos do agente que aí esperam aparecem também
no seu ecrã, e pode responder-lhes aqui (consulte [O que o agente pode fazer](#what-the-agent-can-do)).

### Hosts remotos

No Borg local, prima **Alt-H**, clique em **Hosts remotos** na barra de atalhos ou escolha
**F9 → Remoto → Explorar hosts e sessões…** para ver as máquinas emparelhadas e as respetivas
sessões com nome. Enter abre os ficheiros da sessão selecionada no painel a partir do qual abriu o
seletor, substituindo o que esse painel mostrava. Prima **M** para espelhar o ecrã da sessão,
**U** para atualizar o host (consulte [Atualizar com sessões abertas](#updating-with-sessions-open))
e **R** para atualizar a lista.

**Tab** alterna entre **Hosts** (computadores a que este Borg chega) e **Clientes** (computadores
que chegam a este Borg). Clientes lista os computadores emparelhados com este Borg, as impressões
digitais das respetivas chaves públicas, se estão ligados e quantas ligações tem cada um. Depois de
um computador mudar de chave, a linha da impressão digital indica também em que dia e a impressão
digital que tinha antes. Um computador aparece uma só vez, mesmo que um espelho use várias ligações.
O estado é atualizado enquanto o seletor está aberto, incluindo numa sessão cujas ligações são
mantidas pelo serviço em segundo plano. O emparelhamento funciona num único sentido: um cliente
autorizado não passa automaticamente a ser um host cujos ficheiros este Borg possa explorar. Quando
quem detém a ligação não consegue indicar o estado da ligação, os clientes emparelhados continuam a
ser listados, com **Estado da ligação indisponível**. O mesmo Borg pode ser host e cliente ao mesmo
tempo. O menu Remoto oferece também **Espelhar host montado**, **Partilhar este Borg como host…**,
**Registo remoto…** e **Serviço em segundo plano…**.

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

**F8** ou **Delete** numa sessão de um host remove-a, depois de uma confirmação que tem **Não** por
omissão. Isto fecha a sessão e as respetivas consolas remotas e elimina os painéis do Borg e o
histórico de terminal guardados; os ficheiros e as pastas no host mantêm-se. A sessão Default não
pode ser removida. Uma sessão autónoma que esteja a alojar a ligação não se pode remover a si
própria; feche-a primeiro no host e execute o Borg noutra sessão. Se a remoção falhar, a sessão
continua na lista para poder tentar de novo. Uma sessão em execução é parada primeiro, e o serviço
não a volta a iniciar.

### Espelhar uma sessão

`borg mirror MACHINE:SESSION` mostra o ecrã em direto da sessão remota no seu terminal local;
Alt+Shift+M abre o mesmo espelho a partir de um Borg local com um painel remoto. **Ctrl-G** (mantenha
Control premido e prima G) sai do espelho e volta aos painéis locais, tal como um clique na faixa
na parte inferior. A linha acima dela indica quem tem o controlo; Ctrl-] também sai.

Um `borg` normal ou `borg --session NAME` pode ser espelhado enquanto corre, sem o serviço em
segundo plano. O terminal original continua visível, e qualquer entrada de teclado ou rato nele
retoma o controlo ao espelho. Enquanto tem o controlo, o Borg do host organiza o ecrã para a sua
janela: quando o espaço acima da faixa é mais pequeno do que o terminal do host, o Borg desenha no
canto superior esquerdo desse terminal com o seu tamanho, e o resto fica em branco. Quando o
controlo volta ao terminal do host ou sai do espelho, o Borg volta a ocupar esse terminal inteiro.
Um espelho sem controlo segue o esquema do terminal do host e é cortado para caber numa janela mais
pequena; aumentar a janela mostra o resto. Uma sessão que o serviço em segundo plano mantém continua
ativa depois de o seu terminal fechar, e o computador que tem o controlo define o seu tamanho.

Se outro terminal tiver o controlo da sessão remota, o espelho abre em modo só de visualização e
as suas teclas não chegam à sessão. Prima **Ctrl-T** ou clique na linha de estado para assumir o
controlo e desligar o outro terminal, ou comece com `borg mirror MACHINE:SESSION --take`; `--take`
assume sempre o controlo retirando-o a outro visualizador. Depois de ter o controlo, a primeira
tecla acorda um ecrã remoto em repouso, e as teclas e a entrada de rato seguintes chegam-lhe. Um
visualizador que fique para trás salta os fotogramas de animação antigos e passa diretamente
para o ecrã atual.

Quando o host deixa de responder, por exemplo durante uma atualização, o espelho mantém o último
ecrã, indica-o na linha de estado e tenta de novo a cada 2 segundos, durante 100 segundos no
máximo. Entretanto, **Ctrl-G** sai do espelho. Quando o host regressa, o espelho volta a mostrar o
seu ecrã, com a versão que corre agora no canto inferior direito. Se o host continuar ausente
depois disso, o espelho termina com uma mensagem.

Um pedido do agente aparece no espelho tal como o servidor o mostra, e responde-lhe aí quando
tiver o controlo. Se abriu o espelho a partir de um painel remoto e o pedido ainda esperar quando
sair do espelho, o borg desse painel volta a mostrá-lo.

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

### Tarefas longas e discos

O Borg sabe em que tipo de disco está cada pasta: um disco rígido, um SSD, um disco em RAM, uma
partilha de rede como NFS, SMB ou sshfs, ou a máquina que um painel remoto mostra. No macOS pergunta
ao `diskutil` e ao `hdiutil`, e no Linux lê `/proc/self/mountinfo` e os indicadores do dispositivo
em `/sys`. Ctrl-L mostra o disco e o seu sistema de ficheiros na linha **Disco**, e a linha
**Velocidade** diz a que velocidade o Borg espera que ele leia e escreva.

Antes de uma tarefa escrever o que quer que seja, o Borg calcula quanto tempo vai demorar a partir
dos bytes que lê e escreve em cada disco e do tempo que a compressão leva. Quando uma cópia, uma
movimentação para outro disco ou uma descompactação demoraria mais de 2 minutos, ou uma alteração a
um arquivo mais de 20 segundos, a caixa de diálogo **Tarefa demorada** diz quanto tempo vai levar,
aproximadamente, e em que tipo de disco, e espera:

- **Iniciar** (`s`) corre a tarefa.
- **Sempre para este ficheiro** (`f`) corre-a e deixa de perguntar sobre este arquivo. A
  descompactação e as alterações a um arquivo oferecem esta opção.
- **Sempre neste tipo de disco** (`d`) corre-a e deixa de perguntar sobre trabalho em discos deste
  tipo.
- **Cancelar** (`c`) para antes da primeira escrita.

Uma gravação do editor num arquivo pergunta por cima do editor, que volta a receber as teclas assim
que responder.

Enquanto o Borg não tiver medido um disco, conta com a velocidade habitual do seu tipo: cerca de
120 MB/s para um disco rígido, 900 MB/s para um SSD, 4 GB/s para um disco em RAM, 60 MB/s para uma
partilha de rede e 10 MB/s para uma máquina remota. Cada tarefa que corre durante pelo menos 2
segundos e move pelo menos 64 MiB ensina ao Borg a velocidade real do disco, e a linha **Disco**
passa a dizer **velocidade medida**. Uma tarefa que passa o tempo a comprimir não lhe ensina nada
sobre o disco.

O Borg guarda as velocidades e as suas respostas em `drives.json`, na pasta de estado, com até 256
arquivos. O ficheiro só pode ser lido pelo seu dono, porque indica os arquivos sobre os quais
respondeu. Todos os Borg que tiver abertos partilham-no: cada um lê-o ao arrancar e outra vez mesmo
antes de escrever, por isso o que um Borg aprendeu e as respostas que recebeu mantêm-se quando outro
escreve. A linha
**Tarefas longas** de Ctrl-L diz se o Borg pergunta primeiro nesse disco.
**F9 → Opções → Voltar a perguntar antes de tarefas longas** esquece todas as respostas Sempre,
incluindo as dadas noutros Borg. Um Borg que já esteja aberto mantém as respostas que tem até voltar
a escrever o ficheiro ou até o reiniciar.

### Visualizador e pré-visualização

F3 abre o visualizador. Com o plugin Sistema ativo, F3 num ficheiro `.pcap`, `.pcapng` ou `.cap`
abre antes o visualizador de tráfego do ecrã de rede (consulte
[Capturar tráfego](#capturing-traffic)). Em ficheiros de texto, as setas e PgUp/PgDn percorrem o
texto, `w` ativa ou desativa a moldagem de linhas e Esc ou `q` fecham-no. Em imagens, Left/Right
ou `p`/`n` mostram a imagem anterior ou a seguinte. O Borg usa o protocolo de imagens do terminal
quando existe; nos outros terminais, a imagem aparece como blocos coloridos.

Ctrl-Q mostra uma pré-visualização no outro painel. Tab entra nela, onde as setas, PgUp/PgDn,
Home/End e a roda do rato percorrem o conteúdo; Tab ou Esc voltam à lista de ficheiros. Ctrl-L
mostra informações sobre o ficheiro. F3 abre uma pré-visualização de texto no visualizador completo,
e fechar o visualizador regressa à pré-visualização com a posição, a moldagem, o modo de
código-fonte e as linhas hexadecimais preservados. Ambos leem os ficheiros da mesma forma limitada,
pelo que abrir um ficheiro de texto grande lê apenas mais ou menos a primeira página e continua
rápido, seja qual for o tamanho do ficheiro. Num resultado de uma pesquisa de texto, a
pré-visualização abre em vez disso na primeira correspondência do ficheiro, com a pesquisa marcada
como a barra de pesquisa a marca, e lê apenas à volta dessa correspondência.

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

Prima `h` no visualizador de texto ou na pré-visualização para ver os bytes do ficheiro em linhas
hexadecimais e prima-o de novo para voltar ao texto. Um ficheiro binário, que o Borg reconhece por
um byte a zero nos primeiros 4 KiB, abre em linhas hexadecimais em ambos, e `h` mostra o seu texto.
Cada linha começa com a sua posição no ficheiro e mostra depois os seus bytes em hexadecimal e os
mesmos bytes como texto, com um ponto em cada byte que não é imprimível. As posições e os bytes a
zero aparecem esbatidos. Uma linha tem 16 bytes, ou 8, 4, 2 ou 1 quando não há espaço para 16. O
Borg lê apenas as linhas que estão no ecrã, pelo que as setas, PgUp/PgDn, Space, Home e End
percorrem de imediato um ficheiro de vários gigabytes. A barra de pesquisa também funciona nas
linhas hexadecimais e marca cada correspondência nas duas colunas. Em hexadecimal, os ficheiros
Markdown e JSON mostram os bytes tal como estão escritos.

No visualizador, uma faixa de entropia acompanha o lado direito das linhas hexadecimais. A entropia
indica quão variados são os bytes, de 0 bits por byte para um único byte repetido a 8 para bytes tão
aleatórios como dados comprimidos, cifrados ou empacotados. O texto fica normalmente perto de 4,5, e
o código entre 5 e 6,5. A faixa cobre o ficheiro inteiro de cima a baixo. A barra de cada célula é
tão longa quanto a entropia mais alta na sua parte do ficheiro, esbatida abaixo de 4 bits por byte e
a vermelho e negrito a partir de 7,2, e as células dos bytes que estão no ecrã ficam marcadas como
numa barra de deslocamento. A linha de estado mostra a entropia na página e se é **baixa**,
**típica** ou **alta**. Prima `]` e `[` para saltar para a sequência seguinte ou anterior de blocos
de entropia alta, ou clique na faixa para ir para essa parte do ficheiro.

O Borg lê a entropia em segundo plano, em blocos de 4 KiB. Um ficheiro até 256 MiB é lido por
inteiro, e um maior é amostrado em 65 536 blocos espaçados por igual, pelo que a memória usada é a
mesma seja qual for o tamanho do ficheiro. Entropia alta onde se esperaria código, cabeçalhos ou
tabelas pode indicar dados empacotados ou cifrados ali escondidos. Multimédia comprimida, arquivos,
volumes cifrados e cofres de chaves têm entropia alta do princípio ao fim, por isso leia a faixa à
luz do que o ficheiro deveria conter.

### Arquivos

Prima Enter num arquivo para o abrir como uma pasta. O Borg abre ficheiros cujo nome termina em
`.zip`, `.7z`, `.rar`, `.tar`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tbz2`, `.tbz`, `.tar.xz`, `.txz`,
`.tar.zst` ou `.tzst`, e ficheiros comprimidos únicos terminados em `.gz`, `.bz2`, `.xz` ou `.zst`,
que abrem como uma pasta com esse único ficheiro. Ctrl-PgDn também abre um arquivo, e F3 no próprio
arquivo continua a mostrar os seus bytes no visualizador. O título do painel ganha uma marca como
**▤ zip**, que indica o formato que o Borg encontrou nos primeiros bytes do ficheiro, e o caminho
continua dentro do arquivo. As pastas lá dentro mostram o tamanho do que contêm. Left, Enter em `..`
ou Ctrl-PgUp no topo do arquivo levam-no de volta à pasta que o contém, com o cursor no arquivo.
Quando o arquivo foi eliminado ou já não se lê, subir a partir de qualquer pasta dele também o leva
de volta para lá, e o aviso diz porquê.

Para listar um arquivo, o Borg lê o que ele contém, o que num tar comprimido significa ler o
ficheiro inteiro. Enquanto um arquivo grande é lido, o rodapé do painel mostra até onde o Borg
chegou, e Esc interrompe a leitura e volta atrás. O Borg guarda as listagens dos últimos 4 arquivos
enquanto os seus ficheiros mantiverem o mesmo tamanho e a mesma data, por isso entrar e sair outra
vez é imediato. Os arquivos com mais de 1 000 000 de entradas não abrem.

F3 ou Enter num ficheiro lá dentro descompacta-o para uma pasta privada na pasta de execução do Borg
e abre-o no visualizador. A descompactação corre como uma tarefa, por isso um ficheiro grande mostra
o progresso, e Esc cancela-a. O Borg apaga o ficheiro descompactado assim que o visualizador o abre.

F4 num ficheiro lá dentro abre-o no editor, e Alt-E abre-o no editor do painel. O Borg descompacta o
ficheiro para a mesma pasta privada, e o título do editor mostra o seu caminho dentro do arquivo.
Cada gravação escreve o ficheiro de volta no arquivo como uma tarefa, e o editor avisa disso
("Guardado. A escrever em site.zip."). Quando o arquivo não o aceita, o aviso diz porquê e onde fica
a sua edição.

F5 copia os ficheiros e pastas marcados para fora do arquivo, para a pasta do outro painel ou para a
pasta que escrever na caixa de diálogo Copiar. **Extrair aqui** e **Extrair para o outro painel**,
no menu Ficheiro (F2), descompactam o arquivo inteiro sob o cursor para a pasta deste painel ou do
outro. Em ambos os casos os ficheiros ficam numa pasta desta máquina, e a tarefa aparece em Alt-J
com o progresso, a velocidade e o tempo restante. Pergunta o que fazer com nomes que já existem, tal
como uma cópia, escreve cada ficheiro com um nome temporário até estar completo e para cada ficheiro
no tamanho que o arquivo lhe atribui.

O Borg altera arquivos zip, 7z e tar, comprimidos ou não. F5 ou F6 a partir de uma pasta desta
máquina copia ou move os ficheiros marcados para o arquivo que o outro painel mostra, ou para a
pasta dele que escrever na caixa de diálogo. F7 cria uma pasta no arquivo, e F8 elimina as entradas
marcadas depois de perguntar. F6 para um nome no mesmo arquivo muda o nome das entradas ou move-as
para lá, e F6 para uma pasta desta máquina move-as para fora: são descompactadas lá e depois saem do
arquivo. Quando um nome já existe no arquivo, o Borg pergunta o que fazer tal como numa cópia, e faz
o mesmo quando dois dos ficheiros que copia têm o mesmo nome, como pode acontecer com ficheiros de
resultados de pesquisa ou de uma coleção. Uma mudança de nome para um nome que o arquivo já tem para
com um aviso. Uma movimentação só elimina o que chegou inteiro, por isso um ficheiro que ignore fica
onde estava, tal como um ficheiro que outro posterior com o mesmo nome substituiu.

Cada alteração corre como uma tarefa em Alt-J. O Borg escreve o arquivo alterado ao lado do original
com um nome temporário, volta a lê-lo e só então o põe no lugar do original, com as mesmas
permissões. A releitura verifica a lista de entradas de um zip ou de um 7z e cada entrada de um tar.
Um ficheiro entra tal como está quando o Borg o escreve, por isso um ficheiro que volte a guardar
enquanto a tarefa espera pela sua resposta entra com o conteúdo mais recente. Uma alteração que
falhe ou que cancele deixa o arquivo como estava. As entradas que a alteração mantém são copiadas
tal como estão guardadas: um zip copia os seus dados comprimidos, e um 7z copia cada bloco sólido
que mantém todas as suas entradas e só volta a comprimir os blocos que perdem alguma. Um tar
comprimido é descompactado e comprimido de novo por inteiro, e descompactado mais uma vez para o
reler, por isso alterar um `.tar.xz` grande demora quase tanto como criá-lo e descompactá-lo. Um zip
ou um tar simples que só ganha entradas é copiado e as novas entradas seguem-se à última, e em APFS,
Btrfs, XFS e bcachefs essa cópia é instantânea. Antes de uma alteração que demoraria muito, o Borg
pergunta primeiro (consulte [Tarefas longas e discos](#long-jobs-and-drives)).

Num arquivo zip ou 7z cifrado, as novas entradas são cifradas com AES-256 e com a palavra-passe com
que abriu o arquivo, e um 7z que esconde a sua lista de entradas continua a escondê-la. As entradas
que a alteração mantém ficam como estavam.

Os arquivos RAR e os ficheiros comprimidos únicos, como `.gz`, continuam só para leitura, e
alterá-los avisa disso. Ctrl-Q e **Comprimir…** não funcionam dentro de um arquivo. Um arquivo
dentro de outro arquivo abre depois de o copiar para fora. A shell
fica na pasta que contém o arquivo, e uma sessão guardada reabre essa pasta com o cursor no arquivo.

Os arquivos zip e 7z cifrados pedem a palavra-passe ao abrir. Na caixa de diálogo **Palavra-passe**,
cada carácter que escreve aparece como ruído de interferência que está sempre a mudar e cintila a
cada tecla. O ruído depende apenas de quantos caracteres escreveu. A caixa volta a pedir quando a
palavra-passe está errada, e Esc sai do arquivo. O Borg guarda a palavra-passe em memória enquanto o painel mostra o arquivo e
nunca a escreve em lado nenhum. **Extrair aqui** não tem maneira de a pedir, por isso, num arquivo
cifrado, prima Enter nele, escreva a palavra-passe e copie os ficheiros para fora com F5.

Antes de escrever o que quer que seja, o Borg soma o que as entradas escolhidas ocupam depois de
descompactadas e pergunta primeiro quando isso passa de 1 GiB e de 100 vezes o tamanho do arquivo,
ou de 100 000 ficheiros, que é o aspeto de uma bomba zip. Também pergunta quando o destino tem menos
espaço livre do que os ficheiros precisam, e uma alteração a um arquivo pergunta quando o seu disco
não tem espaço para o novo arquivo ao lado do antigo. **Descompactar mesmo assim** (`u`) avança, e Cancelar
para antes do primeiro ficheiro. As entradas com caminhos absolutos, com `..` no caminho ou que são
ficheiros de dispositivo ficam de fora, com um aviso, e nenhuma entrada ou ligação de um arquivo
consegue fazer o Borg escrever fora da pasta de destino.

O Borg lê zip, 7z e a família tar por si próprio. Os arquivos RAR passam pelo 7-Zip (`7zz` ou `7z`)
ou pelo `bsdtar`, o que o Borg encontrar primeiro, e o macOS já traz o `bsdtar`. Quando nenhum está
instalado, Enter num arquivo RAR propõe instalar o bsdtar. A caixa de diálogo **Instalar bsdtar**
funciona como a do tcpdump em [Capturar tráfego](#capturing-traffic): mostra o comando exato para o
seu gestor de pacotes, e Enter escreve-o na shell do Borg. O Borg também procura o `bsdtar` na pasta
`libarchive` do Homebrew, que o Homebrew deixa fora do `PATH`. Os arquivos RAR cifrados ainda não
abrem.

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
está. Nos resultados de pesquisa e nas coleções, Alt-E abre o editor no outro painel, para que a
lista continue à vista. Alt-E noutro resultado abre-o no lugar do ficheiro que lá está. Quando esse
ficheiro tem alterações por guardar, Alt-E traz o editor dele para a frente e pergunta se as quer
guardar. F6 passa do editor de um painel para o outro painel, e Tab volta ao editor a partir da
lista de ficheiros. O texto copiado com Ctrl-C num editor cola-se com Ctrl-V no outro. Enquanto um
editor tem o foco, a barra de atalhos mostra as teclas do editor, e as teclas do painel voltam com a
lista de ficheiros. Ctrl-O também mostra a consola a partir de um editor, e F9 abre os menus. Esc ou
F10 fecha o editor e devolve ao painel os seus ficheiros; ao sair, o Borg pergunta primeiro por cada
editor de painel com alterações por guardar. A pré-visualização e as informações do ficheiro
precisam do outro painel, por isso esperam até que o editor dele seja fechado. Um editor aberto ao
lado dos resultados devolve-lhes o foco quando fecha e traz de volta a pré-visualização ou as
informações do ficheiro que mostravam.

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
screensaver = true          # one of five animations, picked at random
screensaver-after = 300     # seconds of idle time
update-check = true         # look for a new release once a day
start-service = true        # start the background service when Borg opens
graphics = "auto"           # auto, kitty, sixel, iterm or off
relay = "wss://relay.example.com"  # the relay for borg cloud

[plugins]
git = true
system = true
docker = true
docker-console = true       # the command line follows container panels into the container
agent-log = true            # keep a log of what agents do in each session
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

[folders]
hotlist = ["~/Projects", "/srv/www"]  # pinned with Ctrl-\ and Insert
history = true              # remember the folders the panels open

[network]
proxy-ca = "~/dev-ca"       # the CA folder for captures through Borg's proxy
keep-free-mib = 1024        # stop captures before the disk has less than this free

[watch]
on = false                  # watch what starts, in the background service
freeze = true               # stop a process a rule is sure about
learning-days = 7           # learn what is normal here before freezing anything
alerts-per-minute = 20      # processes written about before Borg only counts them
freezes-per-minute = 5
keep-running = ["backup-agent"]  # programs Borg never freezes

[[watch.allow]]
program = "/bin/sh"         # allowed starts: program, parent, user, or any of them together
parent = "nginx"

[watch.notify.telegram]     # send alerts as they happen, see Alerts off the machine
token = "123456:ABC"
chat = "123456789"
```

Num terminal local com cor verdadeira, o Borg arranca com uma breve animação do logótipo com
efeito de falha (glitch); qualquer tecla ou clique a salta. Por SSH, e em terminais sem cor
verdadeira, abre diretamente nos painéis.

Após `screensaver-after` segundos sem atividade, o Borg mostra um de cinco protetores de ecrã,
escolhido ao acaso: um cubo a girar num campo de estrelas, o logótipo com efeito de falha, um
relógio nas letras de blocos do logótipo que se desfaz a cada novo minuto, o logótipo a rodopiar
até formar uma galáxia em espiral e a regressar, ou o logótipo reconstruído tábua a tábua enquanto
as tábuas antigas formam um segundo logótipo por baixo. Qualquer tecla ou clique traz os painéis
de volta, e essa tecla não faz mais nada. A aprovação pedida por um agente termina o protetor de
ecrã por si, por isso nunca fica à espera por trás dele.

Os temas personalizados ficam em `~/.config/borg-commander/themes/`. Um tema pode estender um dos
temas incluídos:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

### Ficheiros do Borg

As pastas dos painéis, as posições do cursor, a divisão, os modos de listagem e a ordenação ficam
guardados na pasta de estado, `~/.local/state/borg-commander`, ou na pasta indicada por
`$XDG_STATE_HOME` ou `$BORG_STATE_DIR`.

Enquanto corre, o Borg guarda os scripts de integração com a shell e os ficheiros copiados de
contentores na sua pasta de execução, `$XDG_RUNTIME_DIR/borg-commander`, ou em
`borg-commander-<uid>` na pasta temporária. Só o utilizador pode abrir esta pasta; se já existir uma
criada por outra pessoa, ou em que outros utilizadores possam entrar, o Borg ignora-a e usa outro
nome. O ficheiro `TTL` começa com a hora até à qual a pasta está em uso: cada borg em execução adia
essa hora um dia, de hora a hora, pelo que, depois de ela passar, a pasta pode ser eliminada.

**F9 → Opções → Ficheiros do Borg…** mostra cada sítio onde o Borg guarda ficheiros, quanto ocupa
cada um e onde fica:

- **Ficheiros temporários**: scripts de integração com a shell, pedidos de mudança de pasta vindos
  da shell, cópias de contentores, pré-visualizações remotas, ficheiros descompactados de arquivos
  para o visualizador e o editor, e os certificados do proxy de uma captura, na pasta de execução.
- **Sockets**: por onde os agentes e os espelhos chegam às sessões em curso, em
  `/tmp/borg-<uid>-default`, ou numa pasta `/tmp/borg-<uid>-` com o nome da pasta de execução quando
  `$XDG_RUNTIME_DIR` está definida.
- **Caches**: tamanhos das pastas e índices de nomes, na pasta de cache (consulte
  [Análises de tamanho guardadas](#saved-size-scans)).
- **Sessões e histórico**: sessões, saída da shell, registo remoto, histórico de pesquisas e de
  pastas, coleções, computadores emparelhados com as suas chaves e o que o Borg aprendeu sobre os
  seus discos (`drives.json`), na pasta de estado.
- **Definições**: `config.toml` e os seus temas.
- **Capturas**: as capturas de tráfego que o Borg guardou, onde quer que estejam.

Up e Down escolhem uma linha, e uma nota curta por baixo da lista diz o que o sítio escolhido contém
e quanto tempo fica guardado. Enter abre a pasta desse sítio no painel ativo. Esc fecha a janela.

O Borg faz a sua própria limpeza 30 segundos depois de arrancar e, a seguir, de hora a hora. Só
remove nomes que ele próprio cria e nunca nada que tenha mudado nos últimos 10 minutos, pelo que um
Borg que está a arrancar mantém o que acabou de criar. É isto que sai:

- Pastas de outras versões do Borg na pasta de execução, quando já nenhum Borg corre a partir delas
  e, além disso, nenhum Borg estava a correr quando este arrancou ou há 3 dias que nada nelas muda.
- Pedidos de mudança de pasta, ficheiros escritos a meio, pastas de consolas remotas e certificados
  do proxy de capturas de processos do Borg que terminaram.
- Cópias de contentores, pré-visualizações remotas e ficheiros descompactados de arquivos para o
  visualizador e o editor com mais de um dia.
- Sockets em que ninguém responde e pastas de sockets onde já nada responde.
- Análises de tamanho no formato do Borg 0.4 e 0.5, análises que a cache já não lista e índices de
  nomes de pastas que já não são indexadas.
- Pastas de execução que versões antigas do Borg deixaram abertas a outros utilizadores, quando há 3
  dias que nada nelas muda.

As sessões, o histórico, as definições e as capturas ficam guardados até os eliminar. A linha por
baixo da nota diz quanto resta neste momento e o que a última limpeza removeu. F8 remove os restos
depois de perguntar, e a pergunta indica as pastas de onde vêm.

Cada captura que o Borg guarda entra numa lista das 1000 mais recentes. A janela mostra as que ainda
estão no disco em **Capturas guardadas pelo Borg**, com o tamanho e a data, e **com chaves** quando
ao lado de uma está um ficheiro `.keylog` com o mesmo nome. Enter numa captura abre a pasta dela com
o cursor no ficheiro, e aí F8 elimina-a. As capturas guardadas antes de o Borg começar a manter esta
lista não aparecem nela.

`borg storage` mostra os mesmos sítios no terminal e diz o que resta, e `borg storage clean`
remove-o.

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

A CPU é uma média dos últimos segundos em que a utilização mais antiga vai perdendo peso, como no
`top`, pelo que uma lista ordenada por CPU fica parada enquanto um processo trabalha em rajadas
curtas. O resumo e a CPU de todo o sistema no topo do monitor são calculados da mesma forma. **H**
oculta os processos que não usaram CPU nesse tempo, e a linha do filtro indica quantos estão
ocultos. Um processo que fica parado continua na lista até a sua média descer a 0.0. Na árvore,
os processos de que descende um processo ocupado continuam na lista com ele. Prima H outra vez
para voltar a mostrar todos.

| Tecla | Ação |
| --- | --- |
| Enter | Abrir o mapa de memória do processo / passar para a pré-visualização da região |
| `/` | Filtrar por nome ou PID; ↑↓ continuam a mover, Enter mantém o filtro, Esc limpa-o |
| C / M / G / N / P | Ordenar por CPU / RAM / crescimento / nome / PID; prima de novo para inverter |
| I / O | Ordenar por débito de leitura / escrita |
| T | Mostrar ou ocultar a árvore de processos |
| H | Ocultar ou mostrar os processos que não usaram CPU nos últimos segundos |
| Left / Right | Na árvore, recolher ou expandir o ramo do processo selecionado |
| K | Enviar um sinal ou alterar a prioridade |
| E | Procurar entropia alta em todos os processos |
| Alt-N | Mostrar o tráfego do processo selecionado no ecrã de rede |
| Space | Pausar ou retomar a amostragem |
| R | Atualizar a vista atual |
| Esc | Voltar; na lista de processos, fechar o monitor ou regressar ao ecrã de rede |

Clique no cabeçalho de uma coluna para ordenar, e faça duplo clique num processo para abrir o seu
mapa de memória. O resumo mostra a memória residente atual, o valor mais alto observado, a
variação desde o início da monitorização, a CPU, o espaço de endereçamento virtual e um gráfico da
memória residente nos últimos 60 segundos. Os totais de memória usada, em swap, privada e
partilhada aparecem depois de o mapa de memória ser lido, se o sistema os indicar. **Enter** no
painel de memória abre o mapa de memória. O mapa só é lido quando o pede; prima R para o voltar a
ler depois de o processo alterar os seus mapeamentos.

Por baixo do gráfico, **Árvore de processos** lista os processos de que o selecionado descende, o
próprio processo e tudo o que ele iniciou, cada um indentado por baixo daquele que o iniciou e com a
sua memória residente. Quando não cabem todos, a última linha indica quantos ficaram de fora.

Na árvore que **T** mostra, cada processo aparece por baixo daquele que o iniciou, e **▾** assinala
um processo com outros por baixo. **Left** recolhe o ramo do processo selecionado; num processo sem
nada por baixo, Left recolhe o ramo a que ele pertence e seleciona o processo que o iniciou.
**Right** expande um ramo recolhido e, num ramo aberto, passa para o primeiro processo desse ramo.
Um processo recolhido mostra **▸** e quantos processos esconde. Um ramo fica recolhido até o seu
processo terminar. Enquanto um filtro está aplicado, a lista é plana.

### Sinais e prioridade

K abre **Sinal ou prioridade** para o processo selecionado. Prima a letra de uma opção, ou
desloque-se até ela e prima Enter. Esc fecha a caixa de diálogo sem enviar nada.

| Tecla | Opção | O que faz |
| --- | --- | --- |
| T | **Terminar** | Envia TERM, que pede ao processo que saia |
| I | **Interromper** | Envia INT, o mesmo que Ctrl-C no seu terminal |
| H | **Desligar** | Envia HUP; muitos serviços voltam a ler as definições |
| K | **Matar** | Envia KILL, que termina o processo de imediato sem lhe dar tempo para limpar |
| S | **Parar** | Envia STOP, que coloca o processo em pausa |
| C | **Continuar** | Envia CONT, que retoma um processo parado |
| L | **Baixar prioridade** | Sobe o valor nice em 5, até 19, para que espere mais pela CPU quando o sistema está ocupado |
| R | **Subir prioridade** | Desce o valor nice em 5, até -20 |

Imediatamente antes de enviar, o Borg confirma que o PID ainda pertence ao processo que escolheu,
iniciado à mesma hora, para que um processo novo que tenha reutilizado o PID fique intacto. A linha
de estado confirma o que foi enviado e a lista volta a ser amostrada de imediato. A visão geral
indica quando um processo está parado e mostra o seu valor **Nice** quando não é 0.

Pode enviar sinais aos seus próprios processos e baixar a prioridade deles. O sistema recusa o
resto: processos de outro utilizador e subir uma prioridade, que só um administrador pode fazer.
Nesse caso, o Borg oferece **A Administrador**, que envia a mesma opção através do auxiliar
descrito em [Acesso de administrador](#administrator-access). O Borg não envia sinais a si
próprio; F10 fecha-o.

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

As páginas cuja entropia chega a 7,2 bits por byte são desenhadas a vermelho e negrito, ou só a
negrito onde o vermelho seria difícil de ler, seja qual for o seu tipo. Assim, o código empacotado
ou cifrado destaca-se numa região executável, onde o Borg conta como código todas as páginas que não
são zeros nem enchimento. A legenda mostra a sua proporção como **entropia alta**, a linha do cursor
acrescenta a entropia da página sob ele, e a mesma linha termina com a entropia mais alta da região.
Uma nota sob a legenda explica o que a entropia alta e a baixa costumam significar. As caches
comprimidas, as imagens, os arquivos e as chaves também têm entropia alta, por isso veja o que a
região contém antes de suspeitar dela.

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
verificação não consegue ler é contabilizada, e o Borg oferece **A Administrador**. Encare as
conclusões como pistas, porque depuradores, profilers, atualizadores e ferramentas de segurança
deixam os mesmos vestígios.

### Analisar a memória com regras

**Y**, no mapa de memória ou nas correspondências de uma pesquisa, analisa a memória que o próprio
processo preencheu com as mesmas regras de uma análise de ficheiros: as do Borg e as da sua pasta de
regras (consulte [Analisar ficheiros com regras](#scanning-files-with-rules)). Tal como **Cadeias**,
lê a memória que não provém de ficheiros e a memória em que o processo pode escrever; no macOS, as
regiões em que nunca escreveu também são ignoradas. Uma região sem nenhuma página em memória ou na
swap não é lida, e no Linux as tabelas de páginas mostram que páginas o processo nunca tocou, pelo
que essas também não são lidas. As páginas na swap são lidas, porque um payload pode esperar aí. A
memória é lida em blocos de 16 MiB que se sobrepõem em 4 KiB, por isso uma correspondência que passa
de um bloco para o seguinte continua a ser encontrada. Cada regra aparece uma vez por região, na sua
primeira correspondência.

As correspondências ocupam o painel da lista, com os alertas primeiro. Cada uma indica a regra e de
onde vem, **regras do Borg** ou o seu ficheiro, depois os bytes que encontrou e o que a regra
procura, e as linhas por baixo da lista indicam o que mais a poderia explicar. Selecionar uma
correspondência pré-visualiza os bytes à sua volta, com a correspondência realçada. Os ficheiros de
regras deixados de fora aparecem no fim, com a mensagem do compilador. **R** analisa de novo,
**Ctrl-C** interrompe a análise e mantém o que encontrou, e **Esc** volta atrás. Uma análise lê no
máximo 1 GiB, durante até 45 segundos, lista até 2000 correspondências e indica se parou antes do
fim. A memória que não consegue ler é contabilizada, e o Borg oferece **A Administrador**, que corre
a análise no programa auxiliar com a sua pasta de regras.

Uma análise com regras é uma triagem: um olhar rápido sobre um processo ou uma pasta à procura de
padrões que alguém descreveu. Não vigia o sistema, não bloqueia nada nem limpa, que é o papel da
proteção de endpoints. Algumas coisas ficam fora do seu alcance:

- O macOS protege a memória dos seus próprios programas e dos programas criados com o Hardened
  Runtime contra todos os outros programas, administradores incluídos (consulte [Acesso de
  administrador](#administrator-access)).
- No Linux, `kernel.yama.ptrace_scope` decide de quem pode ler a memória. Com 1, como no Ubuntu, só
  pode ler os seus próprios processos filhos, por isso o Borg oferece **A Administrador** para os
  restantes. Com 3, ninguém pode ler outro processo, administradores incluídos.
- Um rootkit de kernel pode esconder memória, ficheiros e processos de todos os programas, incluindo
  o Borg, por isso uma análise limpa prova pouco.
- Um programa comprimido ou cifrado só mostra o seu código real enquanto corre. Analisar a sua
  memória nessa altura vê o que uma análise do seu ficheiro não consegue ver.

O Borg lê a memória sem se ligar como depurador, com `process_vm_readv` no Linux e `mach_vm_read` no
macOS, por isso o processo não vê nenhum tracer, que é o que a maioria das verificações
anti-depuração procura. Os monitores que correm como administrador, como o auditd, as ferramentas
eBPF e os clientes do Endpoint Security do macOS, podem mesmo assim registar as leituras. Uma
análise de ficheiros abre todos os ficheiros que lê, o que `inotify` e `fanotify` reportam. No
Linux, deixa como estava a hora de acesso dos ficheiros que lhe pertencem.

### Análise de entropia de todos os processos

**E** na lista de processos abre a **Análise de entropia**, que lê uma amostra das páginas de todos
os processos à procura de bytes tão densos como código compactado. Prima a letra de uma opção, ou
desloque-se até ela e prima Enter. Esc fecha a caixa de diálogo.

| Tecla | Opção | O que lê |
| --- | --- | --- |
| C | **Código de todos os processos** | As regiões executáveis, onde aparece código compactado ou cifrado |
| M | **Toda a memória de todos os processos** | Todas as regiões legíveis, portanto também dados como chaves e ficheiros comprimidos |
| L | **Últimos resultados** | Nada de novo: mostra outra vez as ocorrências da última análise |

O Borg lê até 1024 páginas de 4 KiB de uma região e 32 768 de um processo, distribuídas
uniformemente. Um processo com muitas regiões recebe menos páginas por região, mas nunca menos de
16. Uma região com pelo menos uma página de 7,2 bits por byte ou mais é uma ocorrência. As
ocorrências ocupam o painel da lista: primeiro o código, depois a memória que o processo escreveu e
por fim os ficheiros mapeados tal como estão no disco, e dentro de cada grupo primeiro a região com
maior percentagem de páginas com entropia alta. O código com 70% ou mais das páginas com entropia
alta aparece em negrito, segundo a mesma regra da pesquisa de [executáveis
compactados](#finding-packed-executables). Cada linha indica o tipo (**código**, **escrita** ou
**ficheiro**), essa percentagem com a entropia mais alta, e o nome do processo, o PID e a região. O
outro painel mostra os endereços, o tamanho e o acesso da região, quantas páginas da amostra têm
entropia alta, onde começa a primeira delas e o que esses bytes costumam ser. São mantidas as 2000
ocorrências mais fortes, e o resumo conta as mais fracas deixadas de fora.

**Enter** numa ocorrência abre o mapa de memória do processo nessa região, com o mapa de páginas
ativo e o cursor na primeira página densa da amostra, para poder ver os bytes à sua volta. **Esc**
volta ao mapa e, de novo, às ocorrências.

O Borg lê 4 processos de cada vez. Uma região sem nenhuma página em memória ou na swap não é lida,
e no Linux as tabelas de páginas mostram que páginas estão em memória, pelo que a análise não toca
em páginas novas nem traz de volta as que estão na swap. As caches que todos os processos mapeiam
da mesma forma ficam de fora: a cache partilhada do dyld no macOS e as páginas que o kernel mapeia
em todos os processos no Linux. Um ficheiro mapeado sem alterações, como uma biblioteca partilhada,
só é lido no primeiro processo que o mapeia.

Uma análise para ao fim de 90 segundos e deixa o resto para **N**, que continua com os processos a
que não chegou. **Ctrl-C** interrompe-a mais cedo e mantém as ocorrências, e **R** analisa de novo
desde o início. Acima das ocorrências, uma linha conta os processos analisados e as ocorrências, e a
seguinte os processos deixados de fora: ilegíveis, protegidos e não alcançados. No macOS, o vmmap
demora alguns segundos a listar as regiões de um processo, por isso analisar todos os processos pode
exigir várias rondas de **N**.

Os processos cuja memória o macOS protege são contados e ignorados, porque ninguém os pode ler. Para
os processos que o Borg não conseguiu ler, oferece **A Administrador**, que os analisa no programa
auxiliar durante no máximo 60 segundos, para que o auxiliar termine dentro do seu limite de 2
minutos mesmo depois de um pedido de palavra-passe demorado. Bytes densos são comuns: caches
comprimidas, imagens, tipos de letra e chaves têm entropia alta por natureza, por isso encare as
ocorrências como pistas e leia os detalhes antes de suspeitar de um processo.

### Acesso de administrador

Quando a leitura de um mapa de memória ou de uma captura está restrita, uma pesquisa não
conseguiu ler parte da memória, uma análise de entropia não conseguiu ler alguns processos, ou o
sistema recusa um sinal ou uma alteração de prioridade, o Borg
oferece **A Administrador**. No macOS, isto abre o pedido
de autorização do sistema; no Linux, usa `pkexec` e precisa de um agente de autorização em
execução. Apenas um auxiliar de curta duração corre com privilégios elevados. O próprio Borg
continua a correr normalmente, e o auxiliar devolve apenas dados do mapa, um número limitado de
bytes ou as ocorrências de uma análise, ou envia o sinal ou a prioridade que escolheu.

O macOS protege a memória dos seus próprios programas (System Integrity Protection) e dos programas
compilados com o Hardened Runtime contra todos os outros programas, incluindo os administradores.
O Borg obtém esta informação a partir da assinatura do programa: os detalhes do processo indicam
**Memória protegida**, uma leitura recusada explica porquê, e **A Administrador** não é oferecido,
porque não ajudaria. Os mapas de memória e os ficheiros abertos destes programas continuam a ser
mostrados. Também não é oferecido quando o Borg já está a correr como administrador. No Linux, a
política de segurança pode ainda assim negar o acesso; o Borg mostra o erro e o monitor de
processos continua a funcionar normalmente.

Um processo é identificado em conjunto com a sua hora de início, pelo que um processo novo que
reutilize um PID não herda o histórico de outro processo. Os mapas de memória, os resultados de
inspeção, as capturas e o histórico têm todos limites de tamanho. O nome Borg Commander fica
visível no canto inferior direito em todo o lado, incluindo a consola, o visualizador, o monitor,
as caixas de diálogo e a proteção de ecrã.

### Alertas

O Borg pode observar o que arranca numa máquina e avisá-lo quando algo parece errado. Ative
**Vigiar: arranques de processos invulgares** em F9 → Opções → Plugins. O observador vive no
serviço em segundo plano que todos os seus Borg na máquina partilham, por isso todos os Borg leem
os mesmos alertas. Esse serviço corre enquanto houver um Borg aberto. Para continuar a observar
sem nenhum terminal aberto, inicie o serviço no arranque do sistema como descreve
[Sessões com nome e serviço em segundo plano](#named-sessions-and-the-background-service).

**Alt-L** ou **Comando → Alertas** abre a página. A lista ocupa o painel ativo, com o alerta mais
recente primeiro, e o outro painel mostra o alerta selecionado por inteiro. Cada linha mostra a
hora, a regra que disparou, o programa, o processo-pai que o iniciou, o utilizador com que correu
e o que é do processo agora. A linha acima da lista conta os alertas e quantos ainda ninguém leu.

Os detalhes dizem numa frase por que a regra disparou e depois o comando, o programa, o
processo-pai, o utilizador, para onde levavam os fluxos padrão e onde o processo se encontra
agora. Em **Árvore de processos** cada pai aparece acima do seu filho, para que possa ler o
caminho de um serviço até ao comando que o preocupa. **Ligações** lista os sockets que o processo
tinha naquele momento, **Hash do programa** é o SHA-256 do seu programa e **Regras** mostra o que
as regras do Borg e as suas encontraram nesse programa (veja
[Analisar ficheiros com regras](#scanning-files-with-rules)).

Um comando com mais de 2 KiB guarda o início e o fim, 2 KiB ao todo, com `…` entre eles. Os comandos
dos processos-pai e os valores das variáveis de pré-carregamento são cortados da mesma forma. Assim os
200 alertas mais recentes cabem no registo, por mais longos que sejam os comandos, e um primeiro
argumento comprido não consegue tirar de vista o fim de um comando. Ctrl-C copia o comando tal como o
registo o guarda.

| Tecla | Ação |
| --- | --- |
| Enter | Mostrar o processo no monitor de atividade |
| C | Deixar um processo congelado continuar a correr |
| K | Terminar o processo, depois de uma pergunta |
| A | Permitir arranques como este a partir de agora |
| `/` | Filtrar as linhas; ↑↓ continuam a mover, Enter mantém o filtro, Esc limpa-o |
| R | Ler o registo de novo |
| Ctrl-C | Copiar o comando que o processo correu |
| Alt-W | Maximizar a lista |
| Esc | Limpar o filtro e depois fechar a página |

O filtro mantém os alertas que contêm todas as palavras que escrever, na regra, no seu nome curto,
no motivo, no programa, no processo-pai, no utilizador, no comando e no que o Borg fez. Ponha `!`
antes de uma palavra para deixar de fora o que ela apanha: `shell-on-socket !root` mantém as
shells remotas de todos os outros utilizadores.

Os alertas vivem em `alerts.json` na pasta de estado, acessível só ao dono, os 200 mais recentes.
O Borg mantém o ficheiro dentro de 8 MiB, o máximo que lê de volta, e deixa de fora os alertas mais
antigos quando não cabem. A janela **Ficheiros do Borg** conta-os em **Estado** (veja
[Ficheiros do Borg](#borgs-files)). Abrir a página marca como lidos os alertas que ela mostra, e a
contagem de alertas não lidos acompanha o registo enquanto o Borg estiver aberto, esteja a página
aberta ou não.

Quando o Borg encontra um `alerts.json` que não consegue ler, como um que uma versão anterior deixou
passar dos 8 MiB, muda o nome do ficheiro para `alerts-unreadable.json` antes de escrever um registo
novo, para que os alertas nele fiquem no disco. Um seguinte recebe um nome numerado como
`alerts-unreadable (2).json`. A página de alertas indica o ficheiro por baixo da lista enquanto ele
lá estiver, por isso apague-o depois de o rever.

### O que o observador procura

Cada regra nomeia uma coisa estranha num processo que acabou de arrancar. O nível diz quanto isso
significa: **alerta** raramente é inocente, **aviso** tem muitas vezes um motivo comum, **nota**
está lá para o retrato ficar completo.

| Regra | O que a faz disparar | Nível |
| --- | --- | --- |
| **Shell num socket de rede** | Uma shell cuja entrada, saída ou erros vão para um socket de rede, que é o aspeto de uma shell remota. Um socket que fica na máquina, como o que o `systemd` dá à saída de um serviço para o journal, não conta | alerta |
| **Shell remota num terminal** | Uma shell num terminal que um programa abriu para ela, em que a entrada, a saída ou os erros desse programa vão para um socket de rede, ou o programa é um retransmissor como o `socat` ou o `nc`. É uma shell remota transformada num terminal completo, como a que o `pty.spawn` do Python cria | alerta |
| **Programa apagado enquanto corre** | O ficheiro do programa já não existe, por isso nada no disco diz o que o processo é | alerta |
| **Programa sem ficheiro** | O Borg não encontra nenhum ficheiro de programa para o processo | alerta |
| **Biblioteca imposta a um programa** | `LD_PRELOAD`, `LD_AUDIT` ou `DYLD_INSERT_LIBRARIES` está definido no seu ambiente | alerta |
| **Shell a partir de um serviço** | Um servidor web, um ambiente de execução de aplicações ou uma base de dados iniciou uma shell | aviso |
| **Shell sem terminal** | Uma shell sem terminal de nenhum dos lados, por isso ninguém a escreveu | aviso |
| **Shell no terminal de um programa** | Uma shell num terminal que um programa abriu para si próprio e em que ninguém iniciou sessão, como o que o `expect` ou o `docker exec` abrem | aviso |
| **Exploração da máquina** | Quatro comandos diferentes como `whoami`, `id`, `uname` e `ss` do mesmo programa no espaço de uma hora, sem ninguém com sessão iniciada no terminal onde correm. Os comandos que ele corre através de uma shell atrás de outra contam juntos, por isso uma web shell que inicia `sh -c` para cada comando também é apanhada | aviso |
| **Programa numa pasta temporária** | O programa está em `/tmp`, `/var/tmp`, `/dev/shm` ou `/run/shm` | aviso |
| **Nome diferente do programa** | O processo chama-se uma coisa e corre outra | aviso |
| **O pai nunca o iniciou antes** | Este pai nunca iniciou este programa nesta máquina | nota |

De cada processo o Borg lê o programa que corre e se o seu ficheiro foi apagado, o comando, o
utilizador, o terminal a que pertence e quem o abriu, para onde levam a sua entrada, saída e erros
padrão, as variáveis de pré-carregamento no seu ambiente e a cadeia de pais acima dele. Um socket
conta como socket de rede quando a rede do processo o lista como TCP ou UDP.

Se alguém iniciou sessão num terminal decide-se da mesma forma que para congelar (veja
[Congelar um processo](#freezing-a-process)): por SSH, na consola, no `tmux` ou no `screen`, através
do `sudo` ou do `su`, ou numa janela de terminal. Um terminal que um programa abriu para si próprio
não conta, quer tenha sido o servidor remoto de um editor, o `docker exec` ou uma shell reversa a
abri-lo.

O Borg aprende a máquina antes de agir. Nos primeiros 7 dias regista qual pai inicia qual programa
aqui e não congela nada, e a coluna Resposta diz **Teria congelado** onde o teria feito. Depois
disso, um par que já viu fica em silêncio para as regras que se podem aprender: as três regras das
shells, o nome diferente e o pai que nunca o iniciou. A série de comandos aprende-se pelo programa de
onde vem: fica em silêncio quando esse programa correu cada um dos seus comandos durante a semana de
aprendizagem, por isso um agente de monitorização que corre `df` e `ps` o dia todo fica em silêncio,
e um servidor web que nunca correu `id` não fica. As cinco regras de nível alerta disparam sempre, porque uma shell num socket ou uma biblioteca imposta nunca se tornam
normais por acontecerem muitas vezes. Tudo o que corre nessa semana conta como normal, por isso
ligue o observador enquanto confia na máquina. Os arranques em que disparou uma regra que a
aprendizagem não consegue silenciar ficam fora do que o Borg aprende.

**A** num alerta permite arranques como aquele a partir daí, pelo seu programa e pelo seu pai. Essas
permissões ficam em `watch-allowed.json` na pasta de estado, que o observador lê e adota no espaço
de um minuto. A secção `[watch]` da configuração guarda o mesmo à mão (veja
[Configuração](#configuration)). O Borg escreve no máximo 20 processos por minuto e congela no
máximo 5, e a linha acima da lista diz quantas deteções deixou de fora quando mais dispararam ao
mesmo tempo. Um processo que o Borg congela fica sempre registado, por muitos alertas que tenham
vindo antes, para que o possa deixar continuar. As notas contam à parte, para que uma série delas
nunca ocupe o lugar de um aviso, e até 20 processos por minuto que o Borg teria congelado além do
limite de 5 também ficam registados.

### Congelar um processo

Terminado o período de aprendizagem, o Borg congela o processo em que disparou uma regra de nível
alerta ou aviso. Congelar envia um sinal de paragem ao processo e a tudo abaixo dele. Um processo
congelado não terminou, por isso o seu serviço continua a correr, o `systemd` e o Docker não têm
nada para reiniciar, e o que ele estava a ponto de fazer espera onde está até você ver. A coluna
Resposta diz **Congelado**, **C** deixa-o continuar e **K** termina-o. Terminar é definitivo, por
isso o Borg pergunta primeiro.

O congelamento começa quando tiver visto todos os alertas da semana de aprendizagem que dizem
**Teria congelado**, ou 3 dias depois de a semana terminar, o que acontecer primeiro. Abrir a página
de alertas marca-os como lidos, e o observador nota-o no espaço de um minuto. Assim pode verificar o
que a semana apanhou e permitir um programa inofensivo com **A** antes de algo parar, e uma máquina
que ninguém vê continua protegida. Até lá, o rodapé da página diz até quando o congelamento espera,
e os detalhes de um novo alerta dizem que está à espera.

O Borg nunca congela o primeiro processo do sistema, nem a si mesmo nem nada que tenha iniciado, nem
um programa da lista que nunca congela (`init`, `systemd`, `sshd`, `sudo`, `su`, `login` e
semelhantes, mais qualquer nome que acrescente em `keep-running`), nem um processo num terminal em
que alguém iniciou sessão, nem um arranque que tenha permitido. Uma nota nunca para um programa. Os
detalhes dizem qual destes motivos manteve o processo a correr.

Um programa da lista incorporada só é poupado onde o sistema guarda os seus programas, como `/usr` e
`/bin`, por isso uma cópia chamada `sshd` noutra pasta é congelada como qualquer outro. Os nomes que
acrescenta em `keep-running` correspondem onde quer que o programa esteja. Um terminal conta quando
alguém iniciou sessão nele: por SSH, na consola, no `tmux` ou no `screen`, através do `sudo` ou do
`su`, ou numa janela de terminal. Um multiplexador de terminal só conta para o root e para contas de
pessoas, numeradas a partir de `UID_MIN` em `/etc/login.defs` (1000 na maioria dos sistemas), porque
uma conta de serviço também pode iniciar um. Um terminal que um programa abriu para si próprio não
conta, o que impede uma shell remota de se esconder atrás de um. Os terminais abertos pelo servidor
remoto de um editor ou pelo `docker exec` também não contam, por isso, se o Borg congelar algo que
correu aí, **C** deixa-o continuar e **A** permite-o.

O Borg verifica a identidade de um processo, o seu número junto com o momento em que arrancou,
imediatamente antes de lhe enviar um sinal, para que um número que outro processo tenha tomado
entretanto fique em paz.

### O observador como root

Sem mais direitos, o Borg sabe apenas dos processos do seu próprio utilizador, olhando para a
lista de processos quatro vezes por segundo, e a página diz isso abaixo da lista. Um comando curto
pode desaparecer antes da olhada seguinte.

Para saber de cada processo no momento em que arranca, instale a parte que corre como root:

```sh
borg service watch install
```

Isto pede a sua palavra-passe uma vez e instala uma unidade separada,
`borg-commander-watch.service`, que corre `borg --watch-root`. Essa parte carrega no kernel pequenos
programas do próprio Borg, que leem cada arranque de programa antes de o programa novo correr: o
programa, a linha de comandos tal como foi passada, o processo que o iniciou e o utilizador. Um
comando que termina de imediato é comunicado na mesma, e também a linha de comandos de um processo
que a reescreve depois. A parte root lê `/proc` para o resto, e para, continua ou termina um
processo quando o Borg pede. Não faz mais nada: a unidade abdica de todas as capacidades de que não
precisa, e a interface do Borg continua a correr como você. Os dois lados falam por um socket na
pasta de estado a que só o dono chega, o Borg aceita uma ligação apenas de um processo que corre
como root, e a parte root liga-se apenas a um socket que pertence ao utilizador para quem foi
instalada. `borg service watch uninstall` remove-a, e `borg service watch status` imprime de onde
vêm os eventos e o que foi encontrado.

Os programas precisam do Linux 5.8 ou posterior. Num kernel mais antigo, ou onde a parte root não os
pode carregar, ela ouve os eventos de processo do kernel e lê tudo de `/proc`, e a página indica-o
por baixo da lista. Depois de atualizar o Borg, execute de novo `borg service watch install`, para
que a unidade receba os direitos de que a nova versão precise.

Os alertas de qualquer dos lados são os mesmos alertas, no mesmo registo. Um processo de outro
utilizador só pode ser congelado pela parte root, por isso retomá-lo ou terminá-lo a partir da
página passa pelo observador no serviço. Quando nada o alcança, a página diz que só o dono ou um
administrador o pode fazer.

O observador é uma funcionalidade de Linux para já. No macOS a página abre e lê o seu registo, e a
origem diz que nada está a observar.

### Alertas fora da máquina

A página Alertas vive na máquina que observa, por isso quem tomar conta dessa máquina pode apagar o
registo. Envie cada alerta para outro lado no momento em que acontece: uma cópia que já chegou fica
fora do seu alcance, e o sinal de vida avisa quando o observador se cala. Adicione `[watch.notify]`
à configuração no servidor, com qualquer um destes destinos ou com vários. Todos os endereços usam
`https://`, ou `http://` para o próprio servidor:

```toml
[watch.notify]
name = "web-01"             # how messages name this machine, its host name otherwise
level = "warning"           # the least an alert needs to be sent: alert, warning or notice

[watch.notify.telegram]
token = "123456:ABC-your-bot-token"
chat = "123456789"

[watch.notify.ntfy]
url = "https://ntfy.sh/a-long-random-topic"
token = "tk_your_token"     # where the topic needs one

[watch.notify.webhook]
url = "https://example.com/borg"
secret = "a long random string"

[watch.notify.heartbeat]
url = "https://hc-ping.com/your-check-id"
minutes = 5
```

Depois execute `borg service watch notify`. Envia uma mensagem de teste a cada destino e mostra o
que cada um respondeu.

- **Telegram**: crie um bot com o @BotFather, que lhe dá o token. Envie uma mensagem ao bot, abra
  `https://api.telegram.org/bot<token>/getUpdates` e tire `chat.id` da resposta.
- **ntfy**: subscreva na aplicação ntfy um tópico com um nome longo que ninguém adivinhe, ou um
  tópico no seu próprio servidor ntfy.
- **Webhook**: o Borg envia cada mensagem para o endereço em JSON. Com `secret`, cada pedido leva
  `X-Borg-Signature: t=<time>,v1=<signature>`: o HMAC-SHA256, sob o segredo, da hora, de um ponto
  e do corpo, para que o recetor distinga as mensagens do Borg das forjadas. `X-Borg-Event` diz o
  que é a mensagem: `alert`, `evidence` (o SHA-256 do programa e as correspondências de regras,
  que seguem um alerta depois de o Borg ler o programa), `started`, `armed` ou `test`.
- **Sinal de vida**: o Borg chama o endereço a cada `minutes` minutos enquanto o observador corre e
  acrescenta `/fail` quando algo está errado: a thread que calcula o hash dos programas parou, um
  destino não aceita mensagens há 30 minutos, o Borg desistiu de uma, ou o Borg não conseguiu ler a
  sua caixa de saída. Um serviço como o healthchecks.io avisa-o quando as chamadas param, que é o
  que acontece quando alguém mata o observador.

O serviço envia cada alerta poucos segundos depois de o registar e diz também quando o observador
arranca e quando o congelamento começa. Os alertas de nível nota ficam só na página, a menos que
`level` seja `notice`. Uma mensagem que um destino recusa espera em `watch-outbox.json`, na pasta
de estado, e volta a ser tentada, 5 segundos depois da primeira vez e a cada 30 minutos no máximo.
O Borg guarda até 500 mensagens em espera, menos quando as longas fariam o ficheiro passar de
16 MiB, e tenta cada uma durante 48 horas. A mensagem seguinte diz de quantas desistiu.

Uma mensagem leva no máximo 1000 bytes da linha de comandos, do caminho do programa e do motivo do
alerta. Uma mais longa guarda o início e o fim, com reticências entre eles, para que encher o início
de um comando não esconda o que ele faz.

Quando o Borg encontra um `watch-outbox.json` que não consegue ler, muda o nome do ficheiro para
`watch-outbox-unreadable.json` e começa uma nova caixa de saída. Um posterior recebe um nome
numerado, como `watch-outbox-unreadable (2).json`. As mensagens que esperavam ficam nesse ficheiro e
nunca são enviadas, por isso o sinal de vida falha uma vez e diz porquê, e a numeração de cada
destino recomeça em 1. Veja o ficheiro e depois apague-o.

Cada destino numera as suas mensagens a partir de 1, e o JSON de cada mensagem leva em `previous` o
SHA-256 da anterior. Um salto na numeração, ou um `previous` que não bate certo, quer dizer que uma
mensagem se perdeu ou foi alterada pelo caminho. A numeração é feita no servidor, por isso não
apanha quem controla o servidor.

Os tokens ficam na configuração, que qualquer coisa a correr como o seu utilizador no servidor pode
ler. Com eles, alguém pode enviar mensagens que parecem do Borg e continuar a numeração sem saltos,
e um bot de Telegram pode apagar o que enviou nas últimas 48 horas. Quem tomar conta do servidor
também pode apagar as mensagens que ainda esperam para sair. O que chegou antes disso fica como
estava, e um webhook para uma máquina que controla guarda-o tal como chegou. Quando o observador se
cala, é o sinal de vida que o avisa.

### O que o observador não consegue ver

O observador avalia cada programa quando ele arranca, o que deixa algumas coisas fora da sua vista.
Conheça-as antes de confiar nele.

- Código que corre dentro de um programa que já está a correr não inicia nada de novo, por isso
  nenhuma regra olha para ele. Uma aplicação web cujo próprio interpretador foi tomado mantém o nome
  e o pai de sempre.
- Os processos que já corriam antes de o observador arrancar nunca são examinados, e tudo o que
  corre durante a semana de aprendizagem conta como normal.
- Sem o observador como root, o Borg vê apenas os seus próprios processos, e um comando que termina
  em menos de um quarto de segundo pode desaparecer antes de o Borg olhar. Os serviços que correm
  como outro utilizador, como `www-data` ou `postgres`, precisam do observador como root.
- Antes do Linux 5.8, ou onde a parte root não consegue carregar os seus programas, o Borg lê cada
  processo novo em `/proc` depois de o kernel o comunicar. Um comando que termina de imediato pode
  já ter desaparecido, e um processo pode mudar antes a sua linha de comandos.
- Alguns kernels não enviam eventos de processos, como em certas máquinas virtuais e ambientes de
  contentores. O Borg a correr como root e o observador como root veem então todos os processos
  quatro vezes por segundo, e um comando curto pode terminar antes de olharem.
- Uma rajada de processos novos pode encher a fila entre o kernel e o Borg, e o kernel descarta
  então eventos, por isso alguns arranques nunca são examinados.
- Um comando que terminou antes de o Borg conseguir ler `/proc` traz apenas o que os programas
  leram: o seu programa, a linha de comandos, o processo pai e o utilizador. O seu terminal, os
  fluxos padrão e as variáveis de pré-carregamento terminaram com ele, por isso as regras sobre eles
  não podem disparar. **Programa apagado enquanto corre** só dispara com ele quando o seu ficheiro
  já não existia ao arrancar, como num programa executado a partir da memória.
- Os programas guardam os primeiros 4 KiB de uma linha de comandos, e o Borg vai buscar uma mais
  longa a `/proc` enquanto o processo ainda lá está. Encontram a linha de comandos e o ficheiro do
  programa através da informação de tipos que o kernel publica em `/sys/kernel/btf/vmlinux`. Num
  kernel compilado sem ela, a linha de comandos também vem de `/proc`. Um comando que terminou antes
  é então identificado pelo caminho com que foi iniciado, que pode ser relativo à sua pasta de
  trabalho ou passar por uma ligação simbólica, por isso **Programa numa pasta temporária** pode não
  o apanhar.
- Tudo o que corre como o utilizador do observador pode parar o observador, alterar os seus
  ficheiros na pasta de estado ou deixar continuar um processo congelado com `kill -CONT`. Congelar
  dá-lhe tempo para ver, e o dono do processo ainda o pode desfazer.
- Um processo num terminal em que alguém iniciou sessão nunca é congelado, mesmo que a sessão tenha
  usado uma palavra-passe ou chave roubada. As regras continuam a disparar e a escrever alertas.
- A **Exploração da máquina** conta os comandos de um programa no espaço de uma hora. Quem espera
  mais entre comandos, ou corre cada um a partir de um programa diferente, fica abaixo dela.
- Um processo que se desliga do seu pai antes de o Borg congelar o pai continua a correr.
- Sem `[watch.notify]`, os alertas só chegam até si quando abre a página. Com ele, quem tomar conta
  da máquina nos segundos depois de um alerta ainda pode travar a mensagem antes de ela sair, e aí
  é o silêncio do sinal de vida que o avisa.

## Rede

**Alt-N** ou **Comando → Rede** mostra o que esta máquina faz na rede: as portas abertas e quem lhes
consegue chegar, os processos que comunicam e a que velocidade, cada ligação e as interfaces de
rede. Faz parte do plugin Sistema incluído, tal como o monitor de atividade. A lista ocupa o painel
ativo, e o outro painel mostra a linha selecionada em pormenor, com gráficos dos últimos 60
segundos. O Borg recolhe uma amostra por segundo enquanto o ecrã está aberto e para quando o fecha.
As velocidades aparecem a partir da segunda amostra, e cada uma é a média dos últimos 3 segundos,
para que uma lista ordenada por tráfego fique parada o tempo suficiente para a ler.

Prima de **1** a **4**, ou clique num separador, para mudar de página:

- **1 Portas** lista cada porta em que um processo escuta, ordenadas por número. **Alcance** indica
  quem se pode ligar: **Todas as redes** para uma porta aberta em todos os endereços, o endereço
  para uma porta associada a endereços específicos, e **Esta máquina** para uma porta a que só os
  programas desta máquina chegam. As portas abertas a todas as redes destacam-se, porque outras
  máquinas as podem encontrar. **Lig.** conta as ligações que entraram pela porta, e os detalhes
  listam-nas com o respetivo tráfego.
- **2 Processos** lista os processos que usam a rede, os mais ativos primeiro, com as velocidades de
  receção e envio e o número de ligações. Os detalhes acrescentam o que o processo transferiu desde
  que o ecrã foi aberto e cada ligação que mantém.
- **3 Ligações** lista cada ligação com o processo, a outra extremidade e as velocidades. Um painel
  largo acrescenta o protocolo e o estado TCP. Os detalhes mostram as duas extremidades, o nome do
  host, o serviço e os bytes recebidos e enviados.
- **4 Interfaces** lista as interfaces de rede com o tipo (**Wi-Fi**, **Ethernet**, **Loopback**,
  **Túnel**, **Ponte** ou **Virtual**), o primeiro endereço e as velocidades. ● marca uma interface
  ativa. Os detalhes acrescentam todos os endereços, a MTU, os bytes e pacotes que a interface
  recebeu e enviou até agora, e os erros.

O rodapé mostra a velocidade total de receção e envio da máquina em todas as interfaces exceto a de
loopback. O Borg procura em segundo plano os nomes dos endereços remotos e mostra o nome em vez do
endereço assim que o conhece.

| Tecla | Ação |
| --- | --- |
| 1 / 2 / 3 / 4 | Mostrar Portas / Processos / Ligações / Interfaces |
| `/` | Filtrar as linhas; ↑↓ continuam a mover, Enter mantém o filtro, Esc limpa-o |
| T / N / P / C | Ordenar por tráfego / nome / porta / ligações; prima de novo para inverter |
| Enter | Mostrar o processo da linha selecionada no monitor de atividade |
| K | Mostrar o processo com **Sinal ou prioridade** aberto |
| R | Capturar o tráfego da linha selecionada ou parar a captura em curso |
| Ctrl-C | Copiar o endereço da porta, o PID, a outra extremidade ou o endereço da interface |
| Tab | Alternar entre a lista e os detalhes |
| Space | Pausar ou retomar a amostragem |
| Alt-W | Maximizar o painel com o foco |
| Esc | Voltar à lista, depois limpar o filtro e depois fechar o ecrã ou regressar ao monitor de atividade |

P ordena as páginas que têm porta, e C as que contam ligações. Clique no cabeçalho de uma coluna
para ordenar por ela, e faça duplo clique numa linha para mostrar o processo. Enter numa linha sem
processo passa para os detalhes. O monitor de atividade que Enter e K abrem tem o processo
selecionado, por isso a memória, os sinais e a prioridade ficam à distância de uma tecla (consulte
[Sinais e prioridade](#signals-and-priority)). Esc aí leva-o de volta ao ecrã de rede, à linha
onde estava e com os gráficos.

Também funciona no sentido inverso. Alt-N no monitor de atividade abre este ecrã na linha do
processo selecionado em **2 Processos**, com o tráfego e as ligações nos detalhes, e Esc regressa
ao monitor. Um processo sem ligações nem portas abertas recebe um aviso que o indica.

### Filtrar

O filtro mantém as linhas que correspondem a todas as palavras que escreve. Uma palavra simples
corresponde ao processo, ao PID, à porta, ao serviço, ao endereço, ao nome do host, ao estado, ao
protocolo ou a uma etiqueta que a linha mostra, por isso `postgres`, `5432`, `https` e `example.com`
funcionam. Uma palavra com nome de campo procura apenas nesse campo:

| Palavra | Mantém |
| --- | --- |
| `port:443`, `port:ssh` | Linhas com esse número de porta ou serviço |
| `proc:node`, `process:node` | Linhas cujo nome de processo contém o texto |
| `pid:4100` | Linhas desse PID |
| `host:example` | Linhas cujo endereço ou nome do host contém o texto |
| `state:listen`, `state:time` | Linhas TCP num estado que contém o texto |
| `proto:udp`, `proto:tcp` | Linhas desse protocolo |
| `if:en0`, `interface:en0` | Interfaces cujo nome contém o texto |

Coloque `!` antes de uma palavra para excluir as linhas que lhe correspondem: `!state:time_wait`
oculta as ligações a fechar, e `!proc:browser` oculta um programa. O filtro aplica-se a todas as
páginas e, enquanto escreve, indica quantas linhas da página mantém.

### Capturar tráfego

Prima **R** para gravar os pacotes da linha selecionada com o `tcpdump`, que vem com o macOS e que no
Linux se instala com o pacote `tcpdump`. A caixa de diálogo **Capturar tráfego** parte dessa linha:

- **Tráfego** é o que o tcpdump guarda. Em **1 Portas** é a porta e as ligações que entraram por
  ela, e em **3 Ligações** essa ligação nos dois sentidos. Em **2 Processos** é **Todo o tráfego,
  mostrado para** esse processo: o tcpdump não consegue escolher pacotes por processo, por isso o
  Borg grava a interface inteira e abre o resultado no processo. **Todo o tráfego da interface**
  está sempre lá como segunda opção.
- **Interface** começa na que tem o endereço da linha ou, se não houver, na mais ativa.
- **Duração** é de 5, 10, 30 ou 60 segundos, ou **Até parar**.
- **Guardar** começa em **Início de cada ligação, até 1 MiB**: os pacotes de cada conversa ficam
  inteiros até transportarem 1 MiB de dados, e a partir daí o Borg guarda só os cabeçalhos. Os
  pedidos curtos ficam completos, e uma transferência longa ou uma videochamada não enchem o
  ficheiro. **Pacotes inteiros** guarda tudo, e **Cabeçalhos, os primeiros 128 bytes de cada
  pacote** guarda o início de cada pacote para os ficheiros mais pequenos. O Borg corta os pacotes à
  medida que escreve o ficheiro, que continua a ser um pcap ou pcapng normal, e cada pacote cortado
  continua a registar o seu tamanho na rede.
- **Chaves TLS** é **Não**, **Definir SSLKEYLOGFILE na shell do Borg** para guardar as chaves
  TLS dos programas que iniciar enquanto a captura decorre, ou **Passar o HTTPS pelo proxy do
  Borg com uma CA temporária** para programas que não escrevem as suas chaves (consulte
  [Desencriptar TLS](#decrypting-tls)).
- **Executar como** aparece onde o Borg pode pedir ao sistema a palavra-passe de um administrador.
  **Utilizador atual** executa o tcpdump com o seu utilizador, e **Administrador** executa-o como
  root depois de introduzir a palavra-passe no diálogo do sistema. No macOS a opção chama-se
  **Administrador, com o processo de cada pacote**: o tcpdump escuta então através do `pktap` do
  macOS, que marca cada pacote com o processo que o enviou ou recebeu.

↑↓ escolhem um campo, ←→ ou Space mudam-no, Enter inicia a captura e Esc cancela. Com o rato, clique
num campo para o escolher e clique outra vez para o mudar. Enquanto a captura decorre, o rodapé
conta os pacotes, os bytes e os segundos que faltam, e **R** para-a mais cedo. Uma captura também
para aos 200 000 pacotes ou 256 MiB, e quando o disco fica cheio, com o que já guardou. Para deixar
espaço para o resto do sistema, defina `keep-free-mib` na secção `[network]` do ficheiro de
configuração: a captura deixa então de começar quando o disco da pasta tem menos desses MiB livres e
para quando o espaço livre desce abaixo disso. Depois dos primeiros 3 segundos, o rodapé diz quanto
falta para o primeiro destes limites parar a captura ao ritmo atual, como **cerca de 30 s até 256
MiB** ou **cerca de 2 min até o disco ficar cheio**, quando isso acontece antes do fim da própria
captura e dentro de uma hora. Quando um limite parou a captura, um aviso diz qual foi. Se fechar o
ecrã antes, a captura para e um aviso diz onde o Borg a guardou.

O Borg guarda cada captura como `capture-<date>-<time>.pcap` na pasta do painel que o ecrã de rede
cobre, legível só por si, para que o Wireshark e outras ferramentas a possam abrir mais tarde. Uma
captura que marca cada pacote com o seu processo é guardada como `capture-<date>-<time>.pcapng`,
o formato que consegue guardar essas marcas. O mesmo acontece a uma captura que guardou chaves
TLS, depois de o Borg as colocar lá dentro. Uma captura contém tudo o que os pacotes transportavam, por isso guarde-a com o mesmo cuidado que esses
dados. Cada captura fica também na lista de [Ficheiros do Borg](#borgs-files).

Um agente ligado através de `borg mcp` também pode pedir uma captura, que aguarda igualmente a sua
aprovação (consulte [O que o agente pode fazer](#what-the-agent-can-do)).

Quando a captura termina, o visualizador de tráfego toma o lugar das páginas ao vivo, com uma página
para cada forma de a ver:

- **1 Processos** mostra cada processo com os hosts com que comunicou, os seus pacotes e os seus
  bytes. Quando os pacotes trazem o seu processo, cada pacote pertence ao processo que o enviou ou
  recebeu. Assim, até um processo que durou menos de um segundo fica com o seu nome, e o filtro de
  um processo mantém exatamente os seus pacotes. Caso contrário, uma conversa pertence ao processo
  cujo socket a tinha durante a captura, e o resto vai para **Processo desconhecido**.
- **2 Hosts** mostra as outras máquinas com os protocolos vistos. Os nomes vêm das respostas DNS da
  própria captura e dos nomes de servidor que as ligações TLS pediram, por isso a maioria dos hosts
  tem um.
- **3 Ligações** mostra cada conversa com o seu processo, a outra ponta, o protocolo, os pacotes e
  os bytes. Os detalhes mostram as duas pontas, o nome do servidor, o serviço, os bytes enviados e
  recebidos, e o fluxo da conversa: o que cada lado enviou, por ordem e com os bytes retransmitidos
  contados uma só vez, num dump hexadecimal com os bytes que se imprimem como texto ao lado. O fluxo
  divide-se em vezes, cada uma com uma seta que indica o sentido, os bytes que contém e a hora a que
  chegou. Quando faltam na captura bytes antes de uma vez, essa vez indica quantos faltam. Quando a
  captura guardou só o início da ligação, o título do fluxo diz quantos bytes ficaram de fora depois
  disso, segundo os cabeçalhos dos pacotes. **]** e **[** saltam para a vez seguinte e para a
  anterior. F3 mostra o fluxo inteiro em linhas hexadecimais no visualizador, com as vezes umas a
  seguir às outras, para o percorrer e pesquisar lá. Enter mostra os pacotes da conversa um a um.
  Quando o Borg tem as chaves TLS da ligação, o fluxo mostra o que os seus registos encriptados
  transportavam (consulte [Desencriptar TLS](#decrypting-tls)).
- **4 Pacotes** mostra todos os pacotes por ordem com o tempo desde o início, o protocolo, o tamanho
  e um resumo como `ClientHello example.com` ou `A example.com → 203.0.113.10`. Os detalhes mostram
  as camadas (Ethernet, IP, TCP ou UDP, e DNS, TLS ou HTTP quando o Borg as lê) e os bytes num dump
  hexadecimal. Um dump põe 16 bytes em cada linha, e menos num painel estreito, para que o texto
  continue a caber ao lado. F3 mostra os bytes do pacote em linhas hexadecimais no visualizador.

| Tecla | Ação |
| --- | --- |
| 1 / 2 / 3 / 4 | Mostrar Processos / Hosts / Ligações / Pacotes |
| Enter | Descer um nível: de um processo ou host para as suas ligações, de uma ligação para os seus pacotes, de um pacote para os seus detalhes |
| ] / [ | Nos detalhes de uma ligação, saltar para a vez seguinte ou anterior do fluxo |
| `/` | Filtrar as linhas, como nas páginas ao vivo |
| Ctrl-C | Copiar o PID, o endereço, a outra ponta ou os bytes do pacote em hex |
| F3 | Mostrar o fluxo da ligação ou os bytes do pacote em linhas hexadecimais no visualizador |
| Tab | Alternar entre a lista e os detalhes, onde ↑↓ percorrem |
| Esc | Voltar à lista, depois subir um nível, depois limpar o filtro e depois voltar ao ecrã ao vivo |

O filtro aceita as mesmas palavras que as páginas ao vivo, e `proto:` também corresponde ao que o
Borg reconheceu nos pacotes: `proto:dns`, `proto:tls`, `proto:http` e `proto:quic`. Um filtro
aplica-se a todas as páginas, e o Enter define um para descer um nível, por isso o Esc consegue
voltar atrás.

F3 num ficheiro `.pcap`, `.pcapng` ou `.cap` num painel abre-o no mesmo visualizador, a começar em
**2 Hosts**. Um ficheiro gravado noutro sítio não tem sockets com que corresponder, por isso as suas
conversas mostram **Processo desconhecido**. Um ficheiro `.pcapng` cujos pacotes trazem o seu
processo, como os que o Borg e o `tcpdump -i pktap` guardam no macOS, continua a mostrar o nome de
cada processo. O visualizador lê ficheiros até 256 MiB e os seus primeiros 200 000 pacotes.

Quando o Borg não encontra o tcpdump, a captura propõe instalá-lo. A caixa de diálogo
**Instalar tcpdump** mostra o comando exato do gestor de pacotes que o Borg encontrou: `apt-get`,
`dnf`, `pacman`, `zypper` ou `apk` no Linux, ou o `brew` do Homebrew no macOS e num Linux sem
nenhum deles. Os gestores de pacotes do sistema são executados com `sudo`, ou com `doas` onde não
houver `sudo`, a menos que o Borg seja executado como root. Enter escreve o comando na shell do
Borg, onde vê o que ele faz e introduz a sua palavra-passe, e Esc cancela. Quando o comando
termina, um aviso diz se correu bem, e Ctrl-O mostra a saída. O Borg volta a procurar o tcpdump
sempre que uma captura começa, no `PATH` e em `/usr/sbin`, `/usr/bin`, `/sbin`, `/bin` e nas pastas
`sbin` do Homebrew, por isso o R funciona logo que a instalação termina. Sem um gestor de pacotes
que o Borg conheça, a captura termina com **Para capturar é preciso o tcpdump, e o Borg não o
encontra.**

O tcpdump precisa de acesso ao dispositivo de captura. O Borg executa-o com o seu utilizador, por
isso dê a si próprio esse acesso uma vez:

- No macOS, de início só o root consegue abrir os dispositivos `/dev/bpf`. Instale uma vez o
  Wireshark com o componente **ChmodBPF**: este entrega os dispositivos a um grupo `access_bpf` e
  acrescenta-o a esse grupo, e a partir daí pode capturar em todas as interfaces.
- No Linux, dê uma vez ao tcpdump as capacidades de captura com
  `sudo setcap cap_net_raw,cap_net_admin=eip /usr/sbin/tcpdump`. Algumas distribuições deixam
  capturar os membros de um grupo `pcap` ou `wireshark`.

Sem acesso, o Borg pergunta **Capturar como administrador** onde o sistema pode pedir a sua
palavra-passe: no macOS no diálogo de palavra-passe do próprio sistema, e no Linux através do
`pkexec`, cujo diálogo vem do seu ambiente de trabalho. Escolha Sim e introduza a palavra-passe no
diálogo do sistema, que o Borg nunca vê. Enquanto o diálogo está aberto, o rodapé mostra **À
espera da palavra-passe para capturar em** e o nome da interface. O Borg executa então uma cópia
de si próprio como root, que verifica o que lhe pediram para capturar e inicia o tcpdump a partir
das pastas do próprio sistema. Os pacotes voltam ao Borg, que guarda o ficheiro com o seu
utilizador. **R** e a duração param a captura como antes, e se o Borg terminar, a captura para com
ele. Se cancelar o diálogo do sistema, a captura termina sem aviso. Para evitar a pergunta,
escolha **Administrador** em **Executar como** antes de começar.

No Linux sem `pkexec`, o Borg usa o `sudo` quando este pode executar o tcpdump sem palavra-passe.
Por SSH não há ambiente de trabalho que mostre o diálogo, por isso a captura termina com **O Borg
não consegue capturar como administrador** e o motivo indicado pelo sistema. Quando o próprio Borg
é executado como root, nunca pergunta, e no macOS as suas capturas trazem sempre o processo de
cada pacote. Onde o Borg não pode perguntar, a captura termina com **Sem permissão para capturar
em** e o nome da interface.

### Desencriptar TLS

Quando o Borg tem as chaves de uma ligação TLS, o fluxo dela em **3 Ligações** mostra o que os
registos encriptados transportavam, como um pedido HTTP e a respetiva resposta, e o cabeçalho do
fluxo termina em **desencriptado**.

As chaves vêm de um ficheiro de chaves TLS, o ficheiro que os browsers, o curl, o Python e muitos
outros programas escrevem quando a variável de ambiente `SSLKEYLOGFILE` indica um. Nesse caso nada
se interpõe entre o programa e o servidor, por isso isto também funciona com programas que fixam
os seus certificados. Para programas que nunca escrevem um ficheiro de chaves, o Borg pode
colocar-se ele próprio no meio (consulte [Pelo proxy do Borg](#through-borgs-proxy)).

Para as guardar com uma captura, escolha **Definir SSLKEYLOGFILE na shell do Borg** em **Chaves
TLS** no diálogo **Capturar tráfego**. Quando a captura começa, o Borg cria
`capture-<date>-<time>.keylog` ao lado da captura, legível só por si, e escreve uma linha como esta
no prompt da sua shell, onde a vê ser executada e a encontra mais tarde no histórico da shell:

```sh
export SSLKEYLOGFILE='/home/me/projects/capture-20260101-120000.keylog'
```

No fish a linha começa por `set -gx`. Depois o Borg volta ao ecrã de rede, onde o rodapé indica
**Chaves TLS** enquanto a captura decorre.

Agora inicie o programa a partir da linha de comando do Borg, ou prima Ctrl-O e inicie-o na shell.
O programa herda a variável e escreve as suas chaves no ficheiro. Para um programa noutro terminal,
prima **L** no ecrã de rede para copiar `SSLKEYLOGFILE='…'` com o caminho do ficheiro de chaves,
cole-o lá e escreva o programa a seguir. Um programa que já estava em execução mantém a definição
com que arrancou, por isso feche-o e inicie-o de novo. Isto vale sobretudo para os browsers, que
passam um segundo arranque à janela que já está aberta. No macOS, o `open` inicia as aplicações sem
as variáveis da shell, por isso inicie o próprio programa da aplicação, como
`/Applications/Firefox.app/Contents/MacOS/firefox`.

Se a shell do Borg estiver ocupada, ou se a linha de comando pertencer a um contentor ou a outra
máquina, o Borg não escreve nada e um aviso indica-o. A tecla **L** continua a copiar a linha.

Quando a captura termina, o Borg escreve `unset SSLKEYLOGFILE` no prompt (`set -e SSLKEYLOGFILE`
no fish), ou repõe o valor com que o próprio Borg arrancou, e um aviso indica a linha. Se um
programa que iniciou ainda estiver em execução na shell, a linha espera que termine. Depois o Borg
coloca as chaves das ligações da captura dentro da captura como um bloco de segredos de
desencriptação, por isso um `.pcap` passa a ser um `.pcapng` com o mesmo nome, que o Wireshark abre
já desencriptado. O ficheiro de chaves fica ao lado, porque os programas iniciados durante a
captura continuam a escrever nele até terminarem. As capturas pedidas por um agente não guardam
chaves.

Leem `SSLKEYLOGFILE` o Firefox, o Chrome e outros browsers baseados em Chromium, a maioria das
compilações do curl e os programas em Python que usam o contexto predefinido do módulo `ssl`.
Outros precisam da sua própria opção:

- O Node.js escreve as chaves com `--tls-keylog`, por isso inicie-o como
  `node --tls-keylog="$SSLKEYLOGFILE" app.js`.
- Os programas em Go só as escrevem quando o programa define `KeyLogWriter` na configuração TLS.
- Os programas em Rust com rustls só as escrevem quando usam o seu `KeyLogFile`.
- O Java precisa de um agente que se ligue ao seu código TLS.

O Safari e outras aplicações criadas sobre as bibliotecas TLS da própria Apple nunca escrevem um
ficheiro de chaves, e o seu tráfego continua encriptado.

#### Pelo proxy do Borg

Defina **Chaves TLS** como **Passar o HTTPS pelo proxy do Borg com uma CA temporária** para ler o
HTTPS de programas que nunca escrevem um ficheiro de chaves, como programas em Go e Rust ou um
cliente que não pode alterar. Quando a captura começa, o Borg corre um proxy em `127.0.0.1` e cria
uma autoridade de certificação (CA) só para esta captura. A chave da CA vive na memória do Borg e
desaparece quando a captura termina. O Borg escreve uma linha no prompt da sua shell que envia o
HTTPS pelo proxy e faz os programas confiarem na CA:

```sh
export HTTPS_PROXY='http://127.0.0.1:54321' https_proxy='http://127.0.0.1:54321' SSL_CERT_FILE='…' CURL_CA_BUNDLE='…' REQUESTS_CA_BUNDLE='…' NODE_EXTRA_CA_CERTS='…'
```

As quatro variáveis de certificados indicam um único ficheiro na pasta de execução do Borg,
legível só por si. Esse ficheiro tem o certificado da CA seguido dos certificados do próprio
sistema, por isso os programas continuam a confiar em todos os outros anfitriões. Enquanto a
captura decorre, o rodapé mostra **Proxy em** e o endereço do proxy, e **L Copiar definições do
proxy** copia as mesmas variáveis para um programa noutro terminal. Quando a captura termina, o
Borg devolve à shell o que ela tinha, tal como faz com `SSLKEYLOGFILE`, e apaga o ficheiro. O Borg
nunca acrescenta a sua CA ao repositório de certificados de confiança do sistema.

Um programa que passa pelo proxy pede-lhe um túnel até ao anfitrião. O proxy liga-se ao
anfitrião, verifica o certificado dele como o sistema o faz e depois mostra ao programa um
certificado para o mesmo anfitrião assinado pela CA. O programa e o anfitrião continuam a falar
HTTP/2 ou HTTP/1.1, conforme o que o anfitrião escolheu. O proxy escreve as chaves das duas sessões
TLS no ficheiro de chaves da captura, por isso a cópia da ligação ao anfitrião que está na captura
abre desencriptada, e o visualizador atribui-a ao processo que pediu o túnel. Tudo o que passa num
túnel sem ser TLS segue tal como está.

Os programas passam pelo proxy quando leem os dois tipos de variáveis:

- O curl, o git e o Python com requests ou urllib seguem `HTTPS_PROXY` ou `https_proxy`, e
  confiam no ficheiro de certificados através de `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, ou
  `SSL_CERT_FILE` quando usam OpenSSL.
- O Go segue `HTTPS_PROXY`, exceto em pedidos para `localhost`, e lê `SSL_CERT_FILE` no Linux. No
  macOS confia apenas nos certificados do sistema, por isso aí as suas ligações falham.
- O Node.js confia na CA através de `NODE_EXTRA_CA_CERTS`. As versões recentes enviam `fetch` e
  `https` pelo proxy quando também define `NODE_USE_ENV_PROXY=1`.
- Programas com a sua própria lista de certificados, programas que fixam certificados e
  aplicações que tiram o proxy das definições do sistema, como o Safari, recusam ou contornam o
  proxy. Uma recusa aparece como **Um programa não confiou no certificado do proxy para** e o
  anfitrião.

Quando o certificado do anfitrião não passa na verificação, o proxy fecha a ligação e um aviso
diz **O Borg não conseguiu verificar** e o anfitrião. É o que acontece com os seus próprios
serviços quando os certificados vêm de uma CA de desenvolvimento em que o sistema não confia, como
a que o [mkcert](https://github.com/FiloSottile/mkcert) cria. Aponte `proxy-ca`, na secção
`[network]` do `config.toml`, para a pasta dessa CA:

```toml
[network]
proxy-ca = "~/Library/Application Support/mkcert"  # mkcert -CAROOT prints the folder
```

A pasta tem de conter o certificado e a respetiva chave como `rootCA.pem` e `rootCA-key.pem`, os
nomes que o mkcert lhes dá, ou como `ca.pem` e `ca-key.pem`. A opção no diálogo passa então a ser
**Passar o HTTPS pelo proxy do Borg com a CA em** e a pasta. O proxy assina com a sua CA, em que
os seus programas já confiam, e também confia nela quando verifica os seus serviços. Use uma CA que
exista apenas para desenvolvimento, porque quem tiver a chave dela pode ler o tráfego de tudo o
que confia nela.

A tecla F3 procura chaves em dois sítios:

- Um bloco de segredos de desencriptação dentro de um ficheiro `.pcapng`, onde o Wireshark também
  as guarda.
- Um ficheiro de chaves ao lado da captura, com o mesmo nome e a extensão `.keylog`, como
  `capture-20260101-120000.keylog` ao lado de `capture-20260101-120000.pcap`.

Para desencriptar uma captura feita de outra forma, execute o programa com a variável definida
durante a captura e, quando a captura estiver guardada, mude o nome do ficheiro de chaves para que
coincida com o da captura:

```sh
SSLKEYLOGFILE=~/tls.keylog curl https://example.com
```

O Borg abre registos TLS 1.3 e TLS 1.2 cifrados com AES-128-GCM, AES-256-GCM ou
ChaCha20-Poly1305, as cifras que os browsers e os servidores atuais acordam entre si. Um registo que
nenhuma chave abre mantém os bytes encriptados, e a sua vez indica **ainda encriptado**. Isso
acontece quando faltam ao ficheiro de chaves algumas chaves da ligação, e depois de bytes que faltam
na captura, porque o Borg não consegue saber onde começa o registo seguinte. O tráfego QUIC continua
encriptado.

Um ficheiro de chaves desencripta todas as ligações para as quais tem chaves, incluindo
palavras-passe, cookies e tokens, e o mesmo faz uma captura com as chaves lá dentro. O Borg torna
ambos legíveis só por si. Faça o mesmo com um ficheiro de chaves que escreva por si com `chmod 600`,
guarde os ficheiros de chaves com o mesmo cuidado que as capturas e apague ambos quando terminar.

### O que o sistema indica

No macOS, o Borg lê cada socket com as contagens de bytes e o processo a partir do `netstat`, por
isso o tráfego aparece tanto para TCP como para UDP. Os sockets dos processos de outros utilizadores
também são listados.

No Linux, o Borg lê os sockets de `/proc/net` e encontra o processo de cada socket através dos
ficheiros abertos dos processos. Sem direitos de administrador só consegue ver os seus próprios
processos, por isso os sockets de outros utilizadores mostram **Processo desconhecido**. As
contagens de bytes de TCP vêm do `ss` quando está instalado; os sockets UDP e as máquinas sem `ss`
mostram **—** nas velocidades, e os detalhes indicam **O sistema não indica contagens de bytes
aqui**. Os totais e as velocidades das interfaces funcionam sempre.

## Itens de arranque

**Alt-U** ou **Comando → Itens de arranque** lista tudo o que está configurado para arrancar
sozinho: no arranque do sistema, no início de sessão, segundo um horário, ou sempre que arranca uma
shell ou uma sessão SSH. A alteração mais recente aparece primeiro, por isso o que foi acrescentado
ou editado há pouco fica no topo. Tal como o monitor de atividade, faz parte do plugin Sistema
incluído. A lista ocupa o painel ativo, e o outro painel mostra o item selecionado em pormenor.

Cada linha mostra quando o ficheiro foi alterado pela última vez, o local e o que executa. Um painel
mais largo acrescenta o proprietário do ficheiro e se o programa está lá: **existe**, ou
**em falta** quando o caminho indicado não existe. Um programa em falta merece atenção, porque
qualquer ficheiro que alguém lá coloque mais tarde corre com os direitos do item. A coluna
**Regras** indica as regras a que o item correspondeu, coloridas pelo nível, e aparece assim que
algum item corresponde a uma. A linha acima da lista conta os itens e quantos corresponderam a uma
regra.

Os detalhes mostram o ficheiro e a linha, o que o item executa, o programa e onde o Borg o
encontrou, quando corre nas palavras do próprio ficheiro (`RunAtLoad`, `@reboot`, um calendário ou
um alvo do systemd), o utilizador com que corre, o proprietário do ficheiro e a hora exata da
alteração. Em **Regras**, cada correspondência diz se foi **no ficheiro** ou **no programa**, com o
que a regra procura, quando corresponde a coisas inofensivas e de onde vem a regra. Um item que o
próprio ficheiro desativa aparece esbatido, e os detalhes dizem
**Desativado no próprio ficheiro, por isso não arranca.**

| Tecla | Ação |
| --- | --- |
| Enter | Fechar o ecrã e mostrar o ficheiro do item no painel |
| F3 / F4 | Ver / editar o ficheiro do item |
| `/` | Filtrar as linhas; ↑↓ continuam a mover, Enter mantém o filtro, Esc limpa-o |
| R | Ler de novo todos os locais |
| Ctrl-C | Copiar o que o item executa |
| Alt-W | Maximizar a lista |
| Esc | Limpar o filtro e depois fechar o ecrã |

Faça duplo clique numa linha para mostrar o ficheiro no painel. O filtro mantém os itens que contêm
todas as palavras que escrever, no local, no nome, no que executa, no caminho do ficheiro, no
proprietário, no utilizador com que corre, em quando corre, em **existe** ou **em falta** e nos
nomes das regras a que correspondeu. Ponha `!` antes de uma palavra para excluir o que ela encontra:
`root !timer` mantém os itens de root que não são timers. Cada local tem também um nome curto para o
filtro e para os agentes, como `launch-daemon`, `systemd-timer` ou `ssh-authorized-key`.

### Onde o Borg procura

No macOS:

- **Agente do utilizador**: `~/Library/LaunchAgents`.
- **Agente launchd** e **Daemon launchd**: `/Library/LaunchAgents` e `/Library/LaunchDaemons`.
- **Agente do sistema** e **Daemon do sistema**: `/System/Library/LaunchAgents` e
  `/System/Library/LaunchDaemons`, no volume de sistema selado.
- **Tarefa cron**: os crontabs em `/usr/lib/cron/tabs`, e `/etc/crontab`.
- **Script periodic**: os scripts diários, semanais e mensais em `/etc/periodic` e
  `/usr/local/etc/periodic`.

No Linux:

- **Serviço** e **Timer**: unidades do systemd em `/etc/systemd/system`,
  `/usr/local/lib/systemd/system`, `/usr/lib/systemd/system` e `/lib/systemd/system`, e as unidades
  para onde as suas pastas `.wants` e `.requires` apontam a partir de qualquer outro lugar.
- **Serviço do utilizador** e **Timer do utilizador**: `/etc/systemd/user`,
  `/usr/local/lib/systemd/user`, `/usr/lib/systemd/user`, `~/.config/systemd/user` e
  `~/.local/share/systemd/user`.
- **Arranque automático**: entradas de ambiente de trabalho em `~/.config/autostart` e
  `/etc/xdg/autostart`.
- **Tarefa cron**: `/etc/crontab`, `/etc/cron.d` e os crontabs em `/var/spool/cron/crontabs` ou
  `/var/spool/cron`.
- **Script cron**: `/etc/cron.hourly`, `/etc/cron.daily`, `/etc/cron.weekly` e `/etc/cron.monthly`.
- **Tarefa anacron**: `/etc/anacrontab`.
- **rc.local**: `/etc/rc.local`.
- **Pré-carregamento**: `/etc/ld.so.preload`, as bibliotecas carregadas em todos os programas que
  arrancam.
- **Arranque da shell**: os scripts em `/etc/profile.d`, além dos ficheiros abaixo.

Em ambos, **Arranque da shell** abrange os ficheiros de arranque do zsh, bash e fish na sua pasta
pessoal, como `~/.zshrc`, `~/.bashrc`, `~/.profile` e `~/.config/fish/config.fish`, e as versões de
todo o sistema em `/etc`. **Chave SSH** lista cada chave em `~/.ssh/authorized_keys` e
`~/.ssh/authorized_keys2` com o seu comentário, e com o comando que impõe quando tem um.

O Borg lê estes locais com as suas permissões. Um local que existe mas está fechado para si aparece
no fundo da lista como **Não foi possível ler** com o seu caminho; execute o Borg como administrador
para ver o que contém. O seu próprio crontab é a exceção: quando a pasta dos crontabs está fechada
para si, como acontece no macOS, o Borg pede-o ao `crontab -l`.

Um programa indicado com um caminho completo ou `~/` fica **em falta** quando lá não há nada. Um
nome sem pasta é procurado em `/usr/local/sbin`, `/usr/local/bin`, `/usr/sbin`, `/usr/bin`, `/sbin`
e `/bin`, onde o cron e o launchd procuram. Um caminho relativo, ou um nome que não está em nenhuma
destas pastas, mostra **—**, porque o item pode definir o seu próprio caminho de procura.

### Regras nos itens de arranque

O Borg aplica as regras que traz e as suas (consulte
[Analisar ficheiros com regras](#scanning-files-with-rules)) ao primeiro 1 MiB de cada ficheiro de
arranque e a cada programa até 64 MiB. Um programa que muitos itens executam é analisado uma só vez.
Os programas dos itens **Agente do sistema** e **Daemon do sistema** não são analisados, porque nada
pode alterar o volume de sistema selado enquanto o macOS está a correr, e os detalhes indicam-no.
Quando um ficheiro de regras não compila, o fundo da lista diz quantos ficaram de fora.
