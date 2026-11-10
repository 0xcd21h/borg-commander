# Guía de usuario de Borg

[Volver a la presentación](../../../README.md)

[Instalación](#install) · [Temas e iconos](#themes-and-icons) · [Teclas](#keys) ·
[Ratón y consola](#mouse-and-console) · [Complemento Git](#git-plugin) ·
[Búsqueda, tareas y visores](#find-jobs-and-viewers) · [Configuración](#configuration)

## Instalación

Descarga un binario desde [GitHub Releases](https://github.com/0xcd21h/borg-commander/releases)
o ejecuta el instalador. Funciona en macOS y Linux, tanto en Intel como en ARM:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

El instalador verifica la suma de comprobación del archivo y coloca `borg` en una carpeta de tu
PATH, así que puedes ejecutarlo enseguida en el mismo terminal. Elige `~/.local/bin` si ya está en
tu PATH; si no, `/usr/local/bin` si tiene permiso de escritura (por ejemplo, como root en un
contenedor), y en último caso usa `~/.local/bin` y lo añade al archivo de inicio de tu shell. Al
terminar, inicia borg; define `BORG_NO_RUN=1` para evitarlo, o `BORG_INSTALL_DIR` para elegir la
carpeta. Si ya tienes instalada la misma versión, el instalador no vuelve a descargarla.

Para compilarlo desde el código fuente necesitas Rust 1.97 o posterior:

```sh
cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

La instalación con Homebrew está lista y funcionará en cuanto se publique el tap:

```sh
brew install 0xcd21h/tap/borg-commander
```

Ejecuta `borg` en la carpeta que quieras explorar. F10 sale, y Ctrl-O vuelve de la consola a los
paneles. Para actualizar, ejecuta `borg update` si descargaste el binario, o usa Homebrew o cargo
si lo instalaste así. `borg --version` muestra la versión instalada.

## Temas e iconos

Elige un tema en F9 → Opciones → Tema. La [galería de temas](../../../README.md#themes) muestra
los seis: Midnight, Aurora, Neon, Classic, Paper y Ember.

Los temas usan colores fijos, de modo que una paleta personalizada en el terminal no cambia su
aspecto. En los terminales sin color verdadero (true color) se usan los colores más parecidos de
la paleta de 256 colores.

Cada archivo tiene un icono. En Ghostty, Borg usa automáticamente los iconos más detallados de
Nerd Font; en los demás terminales se usan por defecto símbolos portables: `/` para carpetas, `@`
para enlaces y `*` para ejecutables. Define `icon-style = "nerd"` para usar los iconos detallados
en otro terminal con una fuente adecuada, o `icon-style = "portable"` para usar siempre los
símbolos portables. El valor predeterminado, `"auto"`, se adapta al terminal y no guarda lo que
detecta como tu preferencia de fuente.

Los archivos de cada versión incluyen **Borg Mono**, una fuente monoespaciada con los iconos de
Nerd Font. Instala sus estilos normal y negrita desde `fonts/`, selecciona Borg Mono en la
configuración del terminal y elige el estilo de iconos `"nerd"`. Borg no puede cambiar la fuente
del terminal por ti. El instalador coloca las fuentes en
`${XDG_DATA_HOME:-~/.local/share}/borg-commander/fonts`; Homebrew las coloca en
`$(brew --prefix)/share/borg-commander/fonts`.

## Colecciones y búsqueda recursiva

El menú Buscar reúne las colecciones y la búsqueda recursiva por nombre o contenido de archivo.
Ambas muestran los resultados en una lista ordenable con la ruta de origen de cada archivo, y ambas
funcionan con el visor y la vista previa rápida.

- **Ctrl-F:** busca en la carpeta actual y en todo lo que contiene. Todas las palabras deben
  coincidir: `main rs` encuentra `main.rs`, y `a | b` encuentra cualquiera de las dos. Una palabra
  en minúsculas no distingue mayúsculas; si tiene alguna mayúscula, la coincidencia es exacta.
  También puedes usar patrones glob como `*.log`, frases entre comillas, rutas con barra como
  `src/lib` y `-word` para excluir coincidencias, además de los filtros `ext:rs,toml`,
  `type:file`, `type:dir`, `type:link`, `size:>10M`, `size:1M..50M`, `mod:<7d` (modificado en la
  última semana), `mod:>1y` e `in:src` (una carpeta en cualquier parte de la ruta). Las búsquedas
  recientes aparecen bajo el formulario, primero las hechas en la misma carpeta; haz clic en una,
  o pulsa Ctrl-P y Ctrl-N en el campo de consulta, para reutilizarla. Los nombres coinciden en
  cualquier alfabeto sean cuales sean las mayúsculas del patrón glob, y las letras acentuadas
  coinciden sin importar cómo las guarde el sistema de archivos. Las coincidencias cercanas a la
  carpeta de búsqueda aparecen primero, mientras se sigue leyendo el resto del árbol.
- **Alt-K:** reúne los archivos marcados, o el archivo actual, en una colección en el otro panel.
  Una colección guarda referencias, así que el contenido de los archivos no se copia.
- **Alt-V:** elige una colección guardada en una lista desplazable que muestra cuántas entradas
  tiene cada una y una estimación de su tamaño. Pulsa F7 o N para crear una. Las colecciones se
  conservan al reiniciar Borg.
- **Esc:** oculta los resultados y conserva el cursor, la posición de desplazamiento, las marcas,
  el filtro y la agrupación.
- **Alt-Shift-R:** vuelve a mostrar los resultados ocultos. Un análisis sigue en marcha mientras
  sus resultados están ocultos.
- **Alt-D:** abre Tamaños de carpeta para buscar carpetas por tamaño total o por número de
  archivos. Los archivos ocultos se incluyen por defecto. Alt-D abre el diálogo de Ctrl-F en
  *Totales de carpeta*; cambia el campo *Buscar* para alternar entre los dos modos. Ctrl-C detiene un análisis.
- **Alt-S:** calcula los tamaños de la carpeta seleccionada. **Alt-Shift-D:** abre un árbol de
  tamaños de la carpeta actual.
- **Alt-W:** maximiza o restaura el panel de resultados activo. Los resultados se abren en un
  panel normal por defecto. Puedes hacer clic en los controles `[+]` (maximizar), `[=]`
  (restaurar) y `[-]` (plegar) de la barra de título. Plegar oculta la vista; Alt-Shift-R la
  recupera con su disposición y su selección.
- **Ctrl-S:** filtra los resultados por nombre o ruta de origen. Un filtro vacío lo muestra todo.

### Buscar dentro de los archivos

Escribe texto en **Con el texto** en el diálogo de Ctrl-F para ver los archivos bajo la carpeta que
lo contienen. El patrón de nombre sigue aplicándose, así que `*.rs` con `fn main` en **Con el
texto** encuentra los archivos Rust que lo tienen. Las opciones junto al campo funcionan como en la
barra de búsqueda: Alt-C distingue mayúsculas y minúsculas (si no, una búsqueda en minúsculas
encuentra cualquier combinación), Alt-W busca palabras completas y Alt-R interpreta el texto como
una expresión regular. También puedes hacer clic en las opciones. Ctrl-P y Ctrl-N en el campo
recuperan las búsquedas recientes de la barra de búsqueda y de búsquedas anteriores. Una búsqueda
de texto solo muestra archivos, así que no admite totales de carpeta ni un mínimo de archivos, y
**Tamaños de carpeta** no tiene el campo **Con el texto**.

Cada resultado muestra la primera línea que coincide, con su número de línea, en la columna
**Coincidencia**, y la línea de estado añade el número de línea a la ruta del archivo. F3 abre el
archivo en el visor en esa coincidencia, con la barra de búsqueda abierta con la consulta, para que
Enter pase a la siguiente, y Ctrl-Q lo muestra ahí en la vista rápida, con la consulta marcada. Los
archivos Markdown y JSON se abren como código fuente en ambos, porque la búsqueda leyó el archivo
tal cual, y `s` cambia a la vista formateada. F4 abre el archivo en el editor de Borg en esa
coincidencia, con la coincidencia seleccionada y la barra de búsqueda abierta con la consulta, y
Alt-E hace lo mismo en el otro panel, para que los resultados sigan a la vista. Borg lee los
archivos en segundo plano, y la línea de contexto cuenta los archivos leídos. Los archivos binarios
(un byte NUL en los primeros 8 KiB) y los de más de 64 MiB quedan sin leer y cuentan como binarios o
demasiado grandes. Ctrl-C detiene la búsqueda, que se para en 20.000 archivos como cualquier
búsqueda.

### Analizar archivos con reglas

Pon **Buscar** en **Coincidencias de reglas** en el diálogo de Ctrl-F para analizar los archivos
bajo la carpeta con reglas YARA: las que incluye Borg y las tuyas. El patrón de nombre, el **Tamaño
mínimo** y **Omitir resultados de compilación, cachés y archivos ignorados** siguen aplicándose. Son
las reglas las que deciden qué buscar, así que el diálogo deja de mostrar **Con el texto** y
**Mínimo de archivos**.

Cada resultado muestra en la columna **Coincidencia** la regla más grave que coincide con el
archivo, cuántas más coinciden y los primeros bytes que encontró esa regla, como texto entre
comillas o en hexadecimal. F3 abre el archivo en el visor en esos bytes, con la barra de búsqueda
rellenada con ellos, y F4 y la vista rápida también se abren ahí. La línea de contexto cuenta los
archivos analizados y las reglas usadas. Los archivos de más de 64 MiB quedan sin leer y cuentan
como demasiado grandes, y el análisis se para en 20.000 archivos como cualquier búsqueda. Ctrl-C lo
detiene.

Las reglas propias de Borg buscan:

- `reverse_shell`: una shell interactiva a través de `/dev/tcp`, `nc` o `socat` ejecutando una
  shell, o una shell lanzada en un pseudoterminal.
- `download_and_run`: un script descargado y enviado directamente a una shell por una tubería.
- `decode_and_run`: base64 decodificado y ejecutado como comando de shell o como código Python.
- `crypto_miner`: la dirección de un pool de minería, o la configuración de un minero común.
- `upx_packed`: un ejecutable empaquetado con UPX, que se desempaqueta a sí mismo en memoria.
- `execve_shellcode`: la ruta de la shell que construye el código de un exploit antes de iniciar una
  shell.
- `ssh_key_planted`: una clave pública añadida a un archivo `authorized_keys`.
- `reboot_job_fetch`: una tarea cron que se ejecuta en cada arranque y descarga, decodifica o
  ejecuta algo desde una carpeta temporal.
- `password_prompt`: un diálogo de AppleScript que pide una contraseña con la respuesta oculta, como
  hacen los programas que roban datos en macOS.
- `launch_agent_from_temp`: un agente o daemon de arranque de macOS que inicia un programa desde una
  carpeta temporal o compartida.
- `preload_write`: una escritura en `/etc/ld.so.preload`, que carga una biblioteca en todos los
  programas de Linux.
- `history_wiping`: el historial de la shell desactivado, o los registros del sistema vaciados.
- `agent_unattended`: un agente de programación con IA iniciado con las aprobaciones desactivadas,
  como hizo el ataque a la cadena de suministro de Nx para buscar secretos en las máquinas.
- `hidden_prompt`: una serie de caracteres de etiqueta Unicode invisibles, que pueden ocultar
  instrucciones para modelos de IA en prompts, archivos de reglas y páginas web.
- `pickle_runs_code`: un pickle, el formato en que Python y PyTorch guardan modelos, que ejecuta un
  comando al cargarse.

Pon tus propias reglas en la carpeta `rules` junto a `config.toml`:
`~/.config/borg-commander/rules`, o la misma carpeta bajo `$XDG_CONFIG_HOME` o `$BORG_CONFIG_DIR`.
Borg lee todos los archivos `.yar` y `.yara` que hay ahí y en las carpetas inferiores, cada uno en
su propio espacio de nombres, así que dos archivos pueden usar los mismos nombres de reglas. Cada
archivo se compila por separado. Un archivo con un error se omite entero, la línea de estado lo
nombra con el mensaje del compilador, la línea de contexto cuenta los archivos omitidos y los demás
archivos siguen funcionando. Como cada archivo se carga por separado, un archivo que usa `include`
también se omite, y los archivos que incluye funcionan por su cuenta.

Las reglas pueden importar los módulos `hash`, `math`, `string`, `time`, `elf`, `macho`, `pe`,
`dotnet` y `dex`; un archivo que importa `magic`, `cuckoo` o `console` se omite. Las variables
`filename`, `filepath` y `extension` contienen el nombre, la ruta y la extensión del archivo
analizado, como esperan los conjuntos de reglas escritos para THOR y LOKI, y `filetype` y `owner`
quedan vacías. El metadato `severity` de una regla fija la importancia de una coincidencia: `alert`,
`critical` o `high` dan una alerta, `warning`, `medium` o `suspicious` una advertencia, y `notice`,
`low` o `info` un aviso. Sin él, un `score` de 80 o más da una alerta, de 60 o más una advertencia,
y por debajo un aviso. Una regla sin ninguno de los dos cuenta como advertencia. Sus campos
`description` y `false_positives` aparecen con cada coincidencia.

Borg no incluye conjuntos de reglas públicos, porque sus licencias varían. Descarga los que te
merezcan confianza en la carpeta de reglas y revisa cada licencia antes de usarlos en el trabajo:

- [YARA Forge](https://yarahq.github.io/) reúne en paquetes probados reglas de muchas fuentes
  públicas; cada regla conserva la licencia de su fuente.
- [signature-base](https://github.com/Neo23x0/signature-base) es gratuito para uso no comercial (CC
  BY-NC 4.0).
- [Los protection artifacts de Elastic](https://github.com/elastic/protections-artifacts) se
  publican bajo la Elastic License 2.0.
- [Las reglas de ReversingLabs](https://github.com/reversinglabs/reversinglabs-yara-rules) se
  publican bajo la licencia MIT.

Las reglas coinciden con patrones de bytes y no saben nada de intenciones. Los instaladores, las
herramientas de seguridad y sus datos de prueba también coinciden, y un archivo con el que no
coincide ninguna regla puede ser malicioso igualmente. Toma las coincidencias como pistas. [Analizar
la memoria con reglas](#scanning-memory-with-rules) explica lo que un análisis no puede ver.

### Encontrar ejecutables empaquetados

Pon **Buscar** en **Ejecutables empaquetados** en el diálogo de Ctrl-F para listar los programas
bajo la carpeta cuyo código es tan denso como los bytes comprimidos o cifrados. Los empaquetadores
y los protectores dejan código así, igual que el malware que oculta su código hasta que se ejecuta.
Borg lee las partes de cada archivo ELF, Mach-O y PE que el cargador mapea como ejecutables,
incluida cada parte de un binario Mach-O universal, y mide la entropía de hasta 1024 bloques de
4 KiB repartidos de forma uniforme por ellas. Un programa aparece en la lista cuando al menos el
70 % de esos bloques llega a 7,2 bits por byte. El código compilado de la forma habitual queda muy
por debajo. El patrón de nombre, el **Tamaño mínimo** y **Omitir resultados de compilación, cachés
y archivos ignorados** siguen aplicándose, y el diálogo deja de mostrar **Con el texto** y
**Mínimo de archivos**.

La columna **Coincidencia** indica la proporción de bloques con entropía alta y la entropía más
alta encontrada, como "97% alta · máx. 8". F3 abre el programa en las filas hexadecimales del visor,
en el primer bloque con entropía alta, con la franja de entropía que describe [Visor y vista
rápida](#viewer-and-quick-view) al lado. La línea de contexto cuenta los programas leídos y los
demás archivos, en los que Borg no sabe encontrar código. Ctrl-C detiene la búsqueda, que se para en
20.000 archivos como cualquier búsqueda. Un programa empaquetado solo muestra su código real
mientras se ejecuta, y el [Análisis de entropía de todos los
procesos](#entropy-scan-of-every-process) examina los programas de esa forma.

### Trabajar con los resultados

Haz clic en el encabezado de una columna para ordenar por ella, y otra vez para invertir el orden.
Los menús Izquierda y Derecha también permiten ordenar, y el menú Buscar añade la ordenación por
carpeta de origen y la agrupación por carpeta o por tipo de archivo. Enter sobre el encabezado de
un grupo lo pliega; Insert marca los archivos coincidentes que contiene. Las marcas se conservan
al ordenar o filtrar, y el pie también cuenta los archivos marcados que el filtro oculta.

F3 y Ctrl-Q abren el archivo original y, tras una búsqueda de texto, lo abren en su primera
coincidencia. La vista rápida sigue al cursor por los resultados y muestra cada archivo en su
coincidencia, con la consulta marcada como la marca la barra de búsqueda. Alt-E abre el archivo en
el editor de Borg en el otro panel, para que los resultados sigan a la vista. Esc o F10 cierra ese
editor y devuelve el foco a los resultados, y la vista rápida vuelve si estaba abierta antes. F5
copia a una carpeta real; si el otro panel es una colección, F5 añade referencias a ella. F6
necesita una carpeta real como destino. En una colección, F8 quita referencias y deja los archivos
intactos; en los resultados de búsqueda, F8 pide confirmación para eliminar los archivos originales.
Shift-F8 también elimina los archivos originales desde una colección, tras confirmarlo. Si marcas
una carpeta y archivos que contiene, la tarea procesa la carpeta una sola vez. **Mostrar original**
abre la carpeta del archivo en el otro panel. Left o Esc ocultan los resultados, y Alt-Shift-R los
recupera tal como los dejaste. Tab cambia de panel incluso si el shell tiene un comando a medio
escribir. El shell siempre permanece en una carpeta real. F10 siempre pregunta antes de salir.

Por ahora, las acciones de Git y de archivos comprimidos solo funcionan en paneles de carpetas
reales. Para usarlas con un archivo de una colección, abre antes su carpeta con **Mostrar
original**.

### Límites de la búsqueda

La búsqueda lee los nombres de archivo y los metadatos en un proceso en segundo plano que puedes
cancelar, y se detiene al llegar a 20 000 coincidencias. La línea de contexto indica si la búsqueda
ha terminado, los errores y las entradas omitidas, y si se alcanzó ese límite. Las carpetas ocultas
solo se recorren cuando **Mostrar archivos ocultos** está activado. La búsqueda no sale del sistema
de archivos donde empezó y no sigue enlaces simbólicos a carpetas. La ventana de búsqueda permite
limitar los resultados a archivos o a carpetas y fijar un tamaño mínimo.

### Totales de carpeta y árbol de tamaños

Elige **Totales de carpeta** para medir todos los archivos que hay bajo cada carpeta antes de
filtrar por nombre, tamaño o número mínimo de archivos. Borg conserva las 20 000 carpetas
coincidentes más grandes y las ordena por tamaño; haz clic en Archivos para ordenarlas por número
de archivos. Usa un patrón como `node_modules` o `*cache*`, o `*` para ver todas las carpetas bajo
la carpeta de inicio.

Los totales suman el contenido de los archivos. Al eliminar una carpeta, el espacio en disco que se libera puede ser otro.
Las carpetas anidadas también cuentan en los totales de sus carpetas superiores. Nunca se siguen
enlaces simbólicos, los análisis no salen del sistema de archivos donde empezaron y una carpeta que
se contiene a sí misma (mediante un montaje bind o una carpeta con enlace duro) se cuenta una sola
vez. Cuando un total está incompleto por uno de estos límites, o porque no se pudo leer o se dejó
fuera una subcarpeta, se muestra con ≥. Los análisis de carpetas solo leen metadatos y no abren
el contenido de los archivos. Los resultados aparecen cuando termina la medición, y puedes cancelar
el análisis en cualquier momento.

**Árbol de tamaños de carpetas** muestra en el panel actual el total de la carpeta de inicio y de
sus subcarpetas. Right despliega una rama, o entra en ella si ya está desplegada; Left pliega una
rama o selecciona su carpeta superior. Enter abre la propia carpeta. Haz clic en la flecha de una
rama para plegarla o desplegarla, o en el encabezado de una columna para ordenar por nombre, tamaño
o número de archivos las carpetas del mismo nivel. Esc oculta el árbol, y Alt-Shift-R recupera sus
ramas, marcas y cursor. Las operaciones con archivos actúan sobre las rutas reales, y si marcas una
carpeta y una subcarpeta suya, una tarea de copia o eliminación solo recibe la carpeta.

### Qué dejan fuera las búsquedas

Mientras **Omitir resultados de compilación, cachés y archivos ignorados** está marcado, las
búsquedas por nombre omiten las carpetas generadas: `node_modules`, `.git`, las cachés y los
entornos virtuales de Python, carpetas como `target` y `dist` que están junto a un archivo de
compilación (`Cargo.toml`, `package.json`) y cualquier carpeta marcada con un archivo
`CACHEDIR.TAG`. También omiten los archivos excluidos por `.gitignore`, `.ignore`, `.rgignore` o
`.borgignore`, y los patrones glob de `[search] exclude`.

Para cambiar lo que se deja fuera, pulsa F4 en el diálogo de búsqueda o haz clic en **F4 Editar**.
Puedes activar o desactivar las carpetas generadas y los archivos de exclusión, y añadir (Ins),
editar (Enter) o quitar (Del) patrones. Los cambios se guardan en el archivo de configuración. Una
carpeta excluida sigue apareciendo si su propio nombre coincide, y la línea de contexto indica
cuántas carpetas se dejaron fuera. Añade `excluded:yes` a una consulta para buscar en todo por una
vez, o `hidden:yes` para incluir los archivos ocultos. Los análisis de tamaño siempre lo leen todo.

En macOS, las búsquedas y los análisis de tamaño que empiezan por encima de
`~/Library/Containers` y `~/Library/Group Containers` los omiten: leer ahí hace que macOS retenga
la lectura hasta que alguien responda si se permite acceder a los datos de otra app. Una búsqueda
que empieza dentro de una de estas carpetas sí la lee.

El historial de búsqueda se queda en esta computadora y solo tú puedes leerlo. Define
`history = false` en `[search]` para dejar de guardarlo.

### Búsqueda instantánea con un índice

**Buscar → Indexar esta carpeta para búsqueda instantánea** mantiene un índice de todos los
nombres bajo la carpeta activa, de modo que Ctrl-F en ella, o en cualquier carpeta que contenga,
responde al instante. La línea de contexto indica cuándo se creó el índice. Borg reconstruye los
índices en segundo plano al iniciarse y cada vez que un índice tiene más de 15 minutos, y el
índice anterior sigue respondiendo hasta que el nuevo está listo. Una búsqueda que no deja nada
fuera, o que usa otras exclusiones, lee las carpetas directamente, igual que una búsqueda de texto,
porque el índice solo guarda nombres. Vuelve a elegir la opción del
menú para dejar de indexar y eliminar el índice. Los índices se guardan en la carpeta de caché,
junto a los análisis de tamaño, e `[index]` en la configuración enumera las carpetas indexadas.

### Contar tamaños en segundo plano

Para medir tamaños sin dejar de usar los paneles de archivos normales, pulsa **Alt-I** o elige
**Buscar → Contar tamaños en segundo plano**. Borg analiza todas las carpetas que hay bajo la
actual, incluidos los archivos ocultos, y completa el total de cada carpeta cuando termina con
ella. Mientras tanto, puedes seguir navegando o usar la consola. Las carpetas que aún se están
midiendo muestran un total creciente marcado con `≥`, y los totales se conservan si sales de una
carpeta y vuelves. El pie del panel muestra el progreso; pulsa **Alt-I** de nuevo, o haz clic en
el pie, para detener el análisis y conservar los totales medidos hasta ese momento. Al iniciar una
tarea de copia, movimiento o eliminación se detiene el análisis de tamaños en curso. Los análisis
terminados comparten su caché con el árbol de tamaños y con Totales de carpeta.

### Análisis de tamaño guardados

Los análisis terminados se guardan en esta computadora. Totales de carpeta y el árbol de tamaños
reutilizan un análisis guardado de la misma carpeta con el mismo ajuste de archivos ocultos.
Cuando vuelves a abrir una carpeta que ya se midió, sus tamaños guardados aparecen al instante,
marcados con `~`, con la fecha de la medición encima del árbol. **Ctrl-R** vuelve a analizar en
cualquiera de las dos vistas de tamaño, y **Ctrl-C** cancela. El árbol anterior sigue visible
mientras se actualiza, y se mantiene si el nuevo análisis se cancela o falla. Los listados normales
de carpetas también muestran los totales guardados cuando existen. Los cambios hechos fuera de Borg
pueden dejarlos desactualizados, y navegar nunca inicia por sí solo un análisis recursivo. Las
tareas de copia, movimiento y eliminación de Borg descartan los análisis guardados que abarcan las
rutas que modificaron.

La caché local de SQLite conserva todas las carpetas medidas de hasta 16 carpetas de inicio. Los
árboles cargan las subcarpetas de una rama al desplegarla. Una rama muestra hasta 20 000 carpetas,
y Totales de carpeta conserva las 20 000 coincidencias más grandes; estos límites de visualización
no detienen el análisis ni cambian los totales. Una fila "Archivos en esta carpeta" recoge los
bytes guardados directamente en una carpeta desplegada. Los errores y las subcarpetas omitidas
marcan los totales como incompletos. La caché está en `~/.cache/borg-commander`, o bajo
`XDG_CACHE_HOME` si está definida; `BORG_CACHE_DIR` cambia la ubicación. La caché solo guarda
mediciones que se pueden volver a generar, y es independiente de las colecciones. Un archivo con
enlaces duros cuenta una vez por cada ruta que tiene. Todos los tamaños miden el contenido de los
archivos, así que lo que un archivo ocupa en disco puede ser más o menos.

## Teclas

La barra de atajos de dos filas está bajo la línea de comandos. La primera fila tiene las teclas de
función; la segunda, la búsqueda, las colecciones y otros atajos que dependen de lo que estés
haciendo. Puedes hacer clic en ambas filas. Los avisos sustituyen a la segunda fila durante un
momento.

| Tecla | Acción |
| --- | --- |
| Up, Down, PgUp, PgDn, Home, End | Moverse |
| Enter, Right | Abrir una carpeta / ejecutar o ver un archivo |
| Left | Ir a la carpeta superior |
| Ctrl-PgDn / Ctrl-PgUp | Abrir la carpeta o el archivo comprimido bajo el cursor / ir a la carpeta superior |
| Tab | Cambiar de panel, o entrar en la vista previa rápida y volver a la lista de archivos |
| Ctrl-O | Mostrar el shell / volver a los paneles |
| Alt-I | Contar tamaños de carpetas en segundo plano / dejar de contar |
| Ctrl-S | Búsqueda rápida en la carpeta |
| Ctrl-\ | Carpetas favoritas: saltar a una carpeta fijada o reciente |
| Ctrl-Q | Vista rápida: previsualiza en el otro panel el archivo bajo el cursor (texto, imagen o carpeta) |
| Ctrl-L | Información: muestra en el otro panel el tamaño, los permisos, el propietario, las fechas, el espacio libre y el disco |
| Insert, Ctrl-T, Shift-Up/Down | Marcar archivos |
| `+` `-` `*` | Seleccionar, deseleccionar o invertir por patrón (`*.rs`, `build/`) |
| Alt-Enter | Poner los nombres marcados (o el actual) en la línea de comandos; en `..`, la ruta de la carpeta superior |
| Ctrl-Insert / Alt-Shift-Insert | Copiar los nombres marcados / sus rutas completas |
| Shift-Enter, Ctrl-J | Insertar un salto de línea en el cursor de la línea de comandos; Enter ejecuta el comando, Ctrl-C lo borra |
| Alt-A / Alt-Shift-A | Poner en la línea de comandos la ruta de la carpeta de este panel / del otro panel |
| Shift-PgUp / Shift-PgDn | Desplazarse hacia atrás y hacia delante por la salida de la consola |
| Shift-Home / Shift-End | Ir a la salida más antigua de la consola / volver al prompt activo |
| Ctrl-P / Ctrl-N, Ctrl-R | Historial del shell: comando anterior / siguiente, búsqueda (lo gestiona tu shell) |
| Alt-. | Mostrar archivos ocultos |
| Alt-R | Recargar el panel activo: su carpeta, resultados de búsqueda o colección |
| Alt-H | Hosts remotos: máquinas emparejadas y sus sesiones |
| Alt-Shift-Left/Right | Cambiar el tamaño de los paneles |
| Ctrl-U | Intercambiar los paneles |
| Alt-O | Abrir la carpeta de este panel en el otro panel |
| Alt-J | Tareas: pausar o cancelar copias, movimientos y eliminaciones en curso |
| Alt-M | Monitor de actividad: procesos y memoria |
| Alt-C | Docker: proyectos de Compose y contenedores |
| Alt-N | Red: puertos, conexiones y tráfico |
| Alt-U | Elementos de inicio: lo que arranca solo, el cambio más reciente primero |
| Alt-L | Alertas: lo que encontró el vigilante de procesos |
| Alt-T | Time Machine: qué dejan fuera las copias de seguridad, en macOS |
| Alt-E | Editar el archivo seleccionado en este panel, en lugar de sus archivos, o en el otro panel junto a los resultados |
| F1 | Acerca de |
| F2 | Menú Archivo |
| F3 | Ver (texto o imagen) |
| F4 / Shift-F4 | Editar en el editor de Borg / en `$VISUAL` o `$EDITOR` |
| F5 / F6 | Copiar / mover al otro panel |
| F7 | Crear una carpeta |
| F8 | Eliminar |
| F9 | Barra de menús |
| F10 | Salir |

Tab, Enter, Home, End, Left y Right controlan los paneles mientras la línea de comandos está
vacía. En cuanto empiezas a escribir un comando, estas teclas pasan al shell. Ctrl-P, Ctrl-N y
Ctrl-R usan el historial de comandos de tu shell. Shift-Enter (o Ctrl-J) inserta un salto de línea
sin ejecutar el comando. Los comandos de varias líneas, escritos o recuperados del historial,
agrandan la zona del prompt y acortan los paneles, para que el cursor siempre quede visible. Todas
las teclas habituales de edición y autocompletado de tu shell siguen funcionando.

Al volver a la carpeta superior, el cursor queda sobre la carpeta de la que vienes, en la misma
fila de la pantalla y con la lista desplazada igual que cuando entraste. Si mientras tanto
aparecieron o desaparecieron archivos, o el panel cambió de altura, el cursor sigue en esa carpeta,
tan cerca de su fila anterior como lo permita la lista.

Alt-Enter escribe los nombres tal como los ve el shell desde la carpeta del panel. En `..` escribe
la ruta completa de la carpeta superior, porque su nombre solo apuntaría dentro de la carpeta
actual. En los resultados de búsqueda y en las colecciones escribe rutas completas, ya que sus
archivos vienen de carpetas distintas.

Lo que copias llega al portapapeles de tu computadora a través del terminal, incluso por SSH y en
un espejo que controlas. Borg también recuerda tus últimas 20 copias: Ctrl-V o Shift-Insert pega
la más reciente en los campos propios de Borg, como los diálogos y la búsqueda en memoria, incluso
donde el terminal no puede acceder a tu portapapeles. En la línea de comandos, Ctrl-V pertenece al
shell, así que pega ahí con el atajo de pegar de tu terminal. El menú Archivo ofrece las dos
acciones de copia para teclados sin tecla Insert. Lo que es demasiado grande para que el terminal
lo transmita se queda en Borg.

Si tu terminal se reserva las teclas F, pulsa **Esc y luego un dígito** en menos de 1,5 segundos:
Esc 1 a Esc 9 equivalen a F1 a F9, y Esc 0 a F10. Por ejemplo, Esc 9 abre los menús y Esc 5 copia.
Mientras Borg espera el dígito, la fila de teclas de función indica qué dígito pulsar. Si hay algo
abierto (un aviso, resultados, la vista rápida o Información), el primer Esc lo cierra y un segundo
Esc cancela. Alt-1 a Alt-0 también funcionan cuando el terminal deja pasar Alt. En macOS, puede que
tengas que activar "Usar Opción como tecla Meta" en tu terminal.

Los atajos también funcionan con distribuciones de teclado no latinas: rusa, ucraniana,
bielorrusa, kazaja, griega, hebrea, árabe, georgiana, coreana y tailandesa. Una letra de esas
distribuciones pulsada con Ctrl o Alt cuenta como la tecla que ocupa el mismo lugar en un teclado
estadounidense, y lo mismo ocurre con una letra pulsada en una pantalla o un diálogo que no espera
texto. Con la distribución rusa activa, Alt-M abre el monitor de actividad y Ctrl-C llega al shell.
La línea de comandos, el editor y los campos de Borg conservan las letras que escribes. Las
preguntas con botones **[ Sí ]** y **[ No ]** aceptan Y y N en cualquier idioma, y también la
primera letra de cada botón en el idioma de Borg, como S para Sí en español.

## Ratón y consola

Haz clic en una fila para llevar el cursor a ella, doble clic para abrirla y clic derecho para
marcarla. Haz clic en la barra de menús o en la barra de atajos para usarlas. La rueda del ratón
desplaza el panel.

Los diálogos también funcionan con el ratón. Haz clic en una fila de una lista para elegirla; en
los selectores de tema e idioma, en Hosts remotos y en Acceso remoto, un segundo clic la confirma.
La rueda desplaza listas como las de temas, ajustes y tareas. Puedes hacer clic en las teclas que
aparecen en el borde de un diálogo: haz clic en **Esc cerrar** para cerrar el diálogo, o en un
botón para responder a una pregunta. Hacer clic fuera de una lista, un selector o Acerca de los
cierra. Los formularios y las preguntas siguen abiertos hasta que respondes.

Ctrl-O alterna entre los paneles y la consola. Antes de que escribas o abras la consola, el shell
se cambia a la carpeta del panel activo. En zsh esto ocurre sin mostrar nada y el prompt se vuelve
a dibujar; en bash y fish verás un breve comando `cd` en la consola. Si abriste la
consola tú mismo, sigue abierta después de los comandos y de Ctrl-C. "Volver a los paneles
después del comando" solo se aplica a los comandos ejecutados desde los paneles. En el prompt del
shell, Option-Left y Option-Right (Alt en Linux) mueven el cursor por palabras.

En la consola, la rueda del ratón desplaza hacia atrás la salida sin tocar el comando que estás
escribiendo. Shift-PgUp y Shift-PgDn desplazan una página, Shift-Home salta a la salida más
antigua y Shift-End vuelve al prompt activo, igual que si empiezas a escribir. Para copiar la
salida visible, selecciónala como permita tu terminal, normalmente arrastrando con Shift pulsado,
y usa su atajo de copiar habitual.

### Saltar a carpetas

Escribe `cd ` en la línea de comandos y Borg muestra justo encima las carpetas a las que puedes
saltar: primero tus favoritas, marcadas con ★, y después las carpetas que abrieron los paneles, de
la más reciente a la más antigua. Cada palabra que añades acota la lista y puede coincidir con
cualquier parte de la ruta tal como la muestra la lista, así que `cd pro bo` encuentra
`~/Projects/borg`. Tu carpeta personal aparece como `~`, así que una palabra de su propia ruta, como
`me` en `/home/me`, no coincide con ninguna carpeta que haya dentro. Una palabra que empieza por `/`
coincide con la ruta completa. Las carpetas cuyo nombre coincide con la última palabra aparecen
primero, junto a la línea de comandos. Pulsa Up para entrar en la lista. Mientras la lista está
visible, Up y Down solo se mueven por ella y se detienen en los dos extremos, así que el cursor del
panel no se mueve. Enter abre la carpeta elegida en el panel activo. Tab la escribe en la línea de
comandos detrás de `cd ` sin ejecutar el comando, con escapes en los espacios y en otros caracteres
que la shell separaría o expandiría, y la lista pasa a las carpetas que contiene. Un clic en una
carpeta también la abre. Para ejecutar tu propio `cd`, sigue escribiendo y pulsa Enter.

Escribe una ruta y la lista empieza por las carpetas que contiene. `cd /Volumes` muestra los
volúmenes montados, y `cd ~/Projects/bo` acota la lista a las carpetas de `~/Projects` con `bo` en
el nombre, primero las que empiezan por esas letras. Cualquier otro comando muestra la misma lista
para una ruta con `/`, como `cp notes.txt ~/Documents/`. Ahí Tab o Enter escribe el nombre de la
carpeta elegida en la ruta, con escapes o dentro de tus comillas, según lo necesite la shell, y la
lista pasa a las carpetas que contiene. Esc oculta la lista y devuelve Up y Down al panel. La lista
vuelve cuando empiezas otra palabra o borras la que estás escribiendo, y Esc justo después de un
espacio la oculta solo hasta que sigues escribiendo. Las carpetas ocultas aparecen al escribir un
punto, o cuando los paneles muestran archivos ocultos. Borg lee hasta 10 000 subcarpetas de una
carpeta, y solo en la línea de comandos de esta máquina; la consola de un contenedor o de una
máquina remota conserva el autocompletado de su shell.

Una línea de comandos que solo cambia de carpeta se ejecuta sin que los paneles salgan de la
pantalla, y el panel activo sigue al shell hasta su nueva carpeta. Si la carpeta no existe, un
aviso lo indica y Ctrl-O muestra el mensaje del shell. Una carpeta que tarda en abrirse, como una
en un recurso de red lento, muestra la consola hasta que termina el comando. Cualquier cosa que
vaya más allá de un `cd`, como `cd src && make`, muestra la consola como siempre.

Ctrl-\ o **Comando → Carpetas favoritas** abre las favoritas con la carpeta del panel activo
elegida. Escribe para acotar la lista, elige con Up y Down y pulsa Enter para abrir la carpeta.
Insert fija la carpeta elegida en las favoritas, así que Ctrl-\ seguido de Insert fija la carpeta
en la que estás. F8 o Delete quita una carpeta de las favoritas o de las carpetas recientes. Esc
borra lo que escribiste y un segundo Esc cierra la ventana. Las favoritas admiten hasta 256
carpetas.

Alt-O o **Comando → Misma carpeta en el otro panel** abre la carpeta del panel activo en el otro
panel, para que ambos muestren el mismo lugar. La carpeta ocupa el lugar de la vista rápida, de
Información o de los archivos de un contenedor o de una máquina remota en ese panel. Si el otro
panel mostraba resultados de búsqueda o una colección, Alt-Shift-R en ese panel los recupera. Desde
resultados de búsqueda, Alt-O abre la carpeta donde empezó la búsqueda. Una colección no tiene
carpeta propia, y las carpetas de contenedores y las remotas se abren en un solo panel, así que en
esos casos Alt-O muestra un aviso, igual que cuando el otro panel tiene un editor. Mientras escribes
un comando, Alt-O pasa al shell.

Las favoritas se guardan en `config.toml`, en la sección `[folders]`. Borg recuerda las últimas 64
carpetas que abrieron los paneles en su carpeta de estado, en un archivo que solo tú puedes leer.
Define `history = false` en `[folders]` para dejar de guardarlas.

## Complemento Git

La compatibilidad con Git es un complemento incluido y está activada por defecto. Su casilla está
en F9 → Opciones → Complementos; al desactivarla se ocultan el menú Git y las columnas de estado, y
se detienen las comprobaciones de estado en segundo plano. Git debe estar instalado y en tu PATH.

Dentro de un repositorio, los paneles muestran la rama actual y un estado de dos caracteres junto a
cada archivo. El carácter izquierdo corresponde al índice (cambios preparados) y el derecho, al
árbol de trabajo. `M` significa modificado, `A` añadido, `D` eliminado, `R` renombrado, `??` sin
seguimiento y `U` un conflicto. Las carpetas resumen los cambios que contienen. Los archivos sin
cambios e ignorados no llevan marca. Fuera de un repositorio, los nombres de archivo aprovechan el
espacio extra.

El menú Git muestra el estado del repositorio, las diferencias sin preparar y preparadas, y los
commits recientes, y permite preparar o quitar de la preparación los archivos marcados (o el
archivo actual si no hay ninguno marcado). Las diferencias y el historial se abren en el visor
normal. Quitar archivos de la preparación no toca tus archivos de trabajo. El estado se actualiza
al cambiar de carpeta, ejecutar comandos o usar acciones de Git, y cada cinco segundos mientras
los paneles están inactivos. **Actualizar estado** en el menú Git lo comprueba de nuevo al
momento.

## Complemento Docker

**Alt-C** o **Comando → Docker** muestra tus contenedores en el panel activo, agrupados por
proyecto de Compose, con los contenedores que no forman parte de ningún proyecto al final. El otro
panel muestra el contenedor seleccionado: su estado, puertos, redes, montajes, comando y política
de reinicio, además del uso en vivo de CPU, memoria, red y disco con un gráfico del último minuto.
Para un proyecto, muestra la carpeta del proyecto, sus contenedores y su uso combinado. Esc cierra
la vista.

| Tecla | Acción |
| --- | --- |
| ↑ ↓, PgUp, PgDn, Home, End | Moverse |
| ← → / Enter | Plegar o desplegar un proyecto; Enter sobre un contenedor pasa a sus detalles |
| S / X / R | Iniciar / detener / reiniciar el contenedor, o los contenedores del proyecto |
| P | Pausar o reanudar |
| F | Explorar los archivos del contenedor |
| E, Alt-2 | Abrir un shell en el contenedor, o volver a su shell abierto |
| Alt-1 | Volver a mostrar el resumen del contenedor, también desde su shell |
| D | Mostrar el uso de disco, o volver a los contenedores |
| `/` | Filtrar por nombre, servicio, imagen o proyecto |
| Tab | Alternar entre la lista y los detalles |
| Alt-W | Maximizar el panel con el foco |

Detener y reiniciar piden confirmación antes. En un proyecto, una acción solo afecta a los
contenedores que cambiaría, así que Iniciar solo inicia los que están detenidos.

### Archivos del contenedor

**F** sobre un contenedor en ejecución cierra la vista de Docker y muestra en ese panel el sistema
de archivos del contenedor, empezando por su directorio de trabajo. El título del panel muestra el
nombre del contenedor. El otro panel sigue en tu carpeta del host, de modo que **F5** copia en
ambas direcciones. **F3** muestra, **F7** crea una carpeta y **F8** elimina dentro del
contenedor. La búsqueda, los tamaños, las vistas previas y la edición solo funcionan en el host.
**Alt-C** vuelve a la vista de Docker, y `..` en la raíz del contenedor devuelve el panel a la
carpeta del host que mostraba antes. Para explorarlo hace falta `sh` en el contenedor, así que no
se pueden listar las imágenes mínimas sin shell.

Mientras un panel muestra los archivos de un contenedor, la línea de comandos lo sigue: se
convierte en el shell de ese contenedor, iniciado en la carpeta del panel, con el nombre del
contenedor al principio del prompt. Los comandos se ejecutan dentro del contenedor, el panel se
actualiza cuando terminan y `cd` ahí mueve el panel. Ctrl-O muestra la consola de ese contenedor,
marcada como **▣ nombre consola**. En un panel del host, la línea de comandos y Ctrl-O vuelven a
pertenecer al host, y lo que hubieras escrito ahí sigue en su sitio. Es el mismo shell que el de
la pestaña Shell del contenedor en la vista de Docker.

Para mantener la línea de comandos en el host, desactiva **Seguir** en
Opciones → Complementos. Así, mientras un panel muestra los archivos de un
contenedor, la línea de comandos se atenúa y se marca como **⌂ host**, y Enter no hace nada hasta
que haya un panel del host activo, de modo que un comando nunca se ejecuta en la máquina
equivocada.

### Shells de contenedor

**E** abre un shell en el contenedor (bash si la imagen lo tiene; si no, `sh`) en la pestaña Shell
del panel de detalles, y le envía tus pulsaciones, teclas de función incluidas. **Alt-1** muestra
el resumen, **Alt-C** vuelve a la lista y la rueda del ratón desplaza hacia atrás la salida del
shell. Un shell sigue en marcha mientras miras otros contenedores, cierras la vista de Docker o
usas la consola del host; su contenedor aparece marcado con `›_` en la lista y se cuenta en el
atajo Alt-C. El shell se cierra cuando sales de él, cuando su contenedor se detiene o cuando Borg
sale (Borg pregunta antes si hay shells abiertos).

El complemento usa la CLI `docker` y su contexto actual, así que funciona igual con Docker Desktop,
OrbStack, Colima y motores remotos. Puedes desactivarlo en Opciones → Complementos.

### Uso de disco y limpieza

**D** en la vista de Docker, o **F9 → Docker → Uso de disco**, muestra lo que Docker guarda en
disco en lugar de la lista de contenedores, con una fila por tipo: **Caché de compilación**,
**Imágenes sin nombre**, **Imágenes sin usar**, **Imágenes en uso**, **Contenedores detenidos**,
**Volúmenes anónimos sin usar**, **Volúmenes con nombre sin usar**, **Volúmenes en uso** y
**Redes sin usar**. Cada fila tiene un recuento, un tamaño y lo que liberaría eliminarlo, y la
línea superior suma todo lo que puedes recuperar. Borg mide con `docker system df --verbose` en
segundo plano, lo que puede tardar en un motor muy ocupado. **R** vuelve a medir, y Borg hace lo
mismo cuando termina el siguiente comando del prompt después de que **F8** haya escrito uno.

Las imágenes sin nombre son compilaciones a las que ya nada nombra, normalmente porque su etiqueta
pasó a una compilación más reciente. Las imágenes sin usar tienen nombre, y ningún contenedor se
creó a partir de ellas. Las imágenes y los volúmenes que usan los contenedores, en ejecución o
detenidos, se quedan hasta que se eliminan esos contenedores, así que sus filas no liberan nada.
Una imagen solo libera las capas que no comparte ninguna otra imagen, por eso su tamaño
recuperable puede quedar muy por debajo de su tamaño.

→ o Enter despliega un tipo en sus elementos, de mayor a menor, y ← lo vuelve a plegar. El otro
panel muestra el elemento seleccionado: las etiquetas de una imagen, su ID, su tamaño compartido,
su fecha de creación y su número de contenedores; el tipo de un registro de la caché de
compilación, si está compartido o en uso y cuándo lo usó una compilación por última vez; la imagen
y el estado de un contenedor; el tamaño de un volumen y los contenedores que lo montan. Los
tamaños que Docker no pudo medir aparecen como `?`, y `/` filtra todos los tipos a la vez.

**F8** escribe el comando de limpieza de la fila seleccionada en el prompt del shell del host y
muestra la consola. No se ejecuta nada hasta que pulses Enter, así que puedes leer el comando,
cambiarlo o borrarlo antes con Ctrl-U. En un tipo, F8 escribe su prune, como
`docker image prune -a` para las imágenes sin usar o `docker volume prune` para los volúmenes
anónimos sin usar, y Docker pide confirmación antes de borrar nada. En un elemento, F8 escribe
`docker rmi`, `docker rm` o `docker volume rm` con ese elemento, o con todas las imágenes,
contenedores o volúmenes marcados. Space o Insert marca un elemento, y `*` invierte las marcas del
tipo seleccionado. Una imagen se elimina por todas sus etiquetas, ya que Docker la borra con la
última, o por su ID cuando no tiene ninguna. Los registros de la caché de compilación se eliminan
juntos con `docker builder prune`. Borg nunca añade `-f` ni `--force`.

Los volúmenes guardan los datos que escribieron los contenedores, como los archivos de una base de
datos, y un volumen eliminado no se puede recuperar. El otro panel lo recuerda junto a cada
comando de volúmenes. Cuando la CLI `docker` tiene más de un contexto, los comandos nombran el que
usó Borg para medir, como en `docker --context staging image prune`, para que lleguen al mismo
motor sea cual sea el contexto que elegiría el shell. Siempre van al shell del host, también
mientras un panel muestra los archivos de un contenedor, porque la vista de Docker mantiene la
línea de comandos en el host.

## Exclusiones de Time Machine

En macOS, **Alt-T** o **Comando → Time Machine** muestra qué deja fuera Time Machine de las copias
de seguridad y te permite cambiarlo. Los paneles marcan con ⊘ los elementos excluidos por sí
mismos, y el pie los cuenta, o indica que la carpeta entera no tiene copia de seguridad. La
Información de **Ctrl-L** muestra cómo trata Time Machine el elemento bajo el cursor.

La ventana tiene tres pestañas; pulsa **1**, **2**, **3** o **Tab**, o haz clic en una pestaña,
para cambiar. **Selección** muestra los elementos marcados, o el que está bajo el cursor.
**Cachés aquí** busca en la carpeta del panel carpetas de caché y de compilación como
`node_modules`, `target`, `.venv` y carpetas con un `CACHEDIR.TAG`, de mayor a menor, y suma
cuánto de ellas sigue incluido en las copias de seguridad. **Todas las exclusiones** muestra cada
exclusión que Borg puede encontrar, por tipo: elementos marcados que Spotlight conoce y los de la
carpeta del panel, rutas fijas, volúmenes excluidos y los lugares que macOS siempre deja fuera.

**Space**, **Insert** o un clic derecho marcan filas, y `*` invierte las marcas. **E** excluye las
filas marcadas, o la que está bajo el cursor, marcando el propio elemento: no requiere contraseña y
la exclusión acompaña al elemento si se mueve, aunque una herramienta que elimina y vuelve a crear la
carpeta la pierde. **P** excluye la ruta, que queda excluida sea lo que sea lo que
haya en ella, y pide tu contraseña en el diálogo del sistema. **I**, **Delete** o **F8** vuelven a
incluir elementos. Para volver a incluir en las copias un elemento que está dentro de una carpeta
excluida, incluye la carpeta. Lo que macOS deja fuera por su cuenta no se puede cambiar.

En Todas las exclusiones, **A** excluye una ruta que escribas, incluso una donde todavía no existe
nada. **R** vuelve a analizar, **Enter** o un segundo clic en una fila muestra el elemento en el
panel activo, y **Esc** cierra la ventana. Excluir o incluir un volumen también pide tu
contraseña. Para indicar cómo trata algunas carpetas, puede que Time Machine necesite que tu
terminal tenga acceso total al disco en Ajustes del Sistema › Privacidad y seguridad; hasta
entonces, esas carpetas aparecen como Desconocido. En otros sistemas, la opción del menú está
oculta.

## Claude y otros agentes (MCP)

`borg mcp` permite que un agente de IA como Claude trabaje en el borg que tienes abierto, ya sea en
tu propia máquina o en un servidor al que accedes por SSH. No necesitas instalar nada aparte de
borg: el propio agente inicia `borg mcp` (a través de SSH en el caso de un servidor), y este se
conecta a la sesión de borg que se está ejecutando allí. El servidor no necesita puertos abiertos
ni acceso a internet.

### Conectar Claude

Pulsa F1 y luego C (o elige F9 → Remoto → Compartir este Borg como host…). Borg muestra dos
comandos para pegar en un terminal de tu computadora: uno inicia Claude Code ya conectado a este
borg, y el otro añade la conexión a Claude Code para todas las sesiones. Tab elige uno, y Enter lo
copia al portapapeles de tu computadora a través del terminal. Esto funciona por SSH en iTerm2,
Ghostty, kitty, WezTerm y Alacritty (en tmux, añade `set -g set-clipboard on`). Donde el terminal
no lo acepta, como en muchos terminales del navegador, F3 muestra el comando solo, como texto
plano, para que puedas seleccionarlo con el ratón; cualquier tecla vuelve a borg.

Por SSH, borg rellena la dirección a la que te conectaste. Si tu computadora llega al servidor con
otro nombre, como un alias de SSH o a través de un host de salto, escribe ese nombre.
SSH debe iniciar sesión sin pedir contraseña, con una clave o un agente SSH. `borg mcp setup prod`
muestra la misma configuración para Claude Code y Claude Desktop desde la línea de comandos.

### Qué puede hacer el agente

Una vez conectado, el agente ve lo mismo que tú: la propia pantalla como texto, tanto si estás en
los paneles como en la consola, ambos paneles, los archivos resaltados y marcados, dónde se ejecuta
la línea de comandos y lo que hay escrito en ella. Se refiere a los sitios como ubicaciones: una
ruta absoluta o `~/…` en la máquina donde se ejecuta borg, `docker://web/etc/nginx` dentro de un
contenedor, o una ruta relativa al panel activo. Sin preguntar, puede leer carpetas y archivos (por
rango de líneas, o solo las líneas que contienen cierto texto), ver los procesos más activos y los
contenedores, y mostrar un archivo o una carpeta en tu otro panel. También puede listar los puertos
abiertos, las conexiones y las interfaces de red con su tráfico, medido durante un segundo y acotado
con las mismas palabras que el filtro de la pantalla de red (consulta [Filtrar](#filtering)).
También puede listar los elementos de inicio, con el cambio más reciente primero, con sus programas
y las reglas con las que coincidieron, acotados con las mismas palabras que el filtro de esa
pantalla (consulta [Elementos de inicio](#startup-items)).

Todo lo que modifica la máquina espera tu respuesta en un diálogo que muestra el cambio completo:
un comando y dónde se ejecutará, un archivo nuevo o una edición en forma de diff. Desplaza los
largos con las flechas, PgUp y PgDn, o la rueda del ratón, y pulsa F3 para ver el diálogo a
pantalla completa. La opción No sigue seleccionada hasta que eliges Sí, y una edición se rechaza si el
archivo cambió mientras decidías. El diálogo aparece encima de todo, incluidos el editor, el visor,
otro diálogo y el salvapantallas, y recibe todas las teclas hasta que respondas. Lo que tapaba vuelve
a aparecer después. Si el agente deja de esperar antes, el diálogo se cierra y un aviso lo indica.
Con una computadora emparejada, `approval_mode` te pregunta una
vez antes de activar Aprobar todo para esa sesión. Mientras está activo, se mantiene visible una
insignia; haz clic en ella o pulsa Ctrl+Alt+A para desactivarlo al instante. También se desactiva
cuando se cierra esa conexión emparejada. Las aprobaciones de emparejamiento son aparte.

Una solicitud también llega a tu propia computadora cuando un panel remoto en ella muestra la
sesión en la que espera (consulta [Paneles remotos](#remote-panels)). Tu borg la muestra en el
mismo diálogo, con el nombre del servidor en el título, por encima de lo que estés haciendo, y
envía tu respuesta de vuelta por la conexión cifrada. El servidor también mantiene abierto su
propio diálogo. Cuenta la primera respuesta, venga de donde venga, y los demás diálogos se cierran
con un aviso que dice dónde se respondió. Una solicitud que el agente retira se cierra en todas
partes. Cuando otra computadora controla la sesión a través de un espejo, tu diálogo muestra la
solicitud sin Sí ni No, y Esc la cierra solo en tu pantalla. Mientras esa sesión lo apruebe todo,
tu borg muestra una insignia con el nombre del servidor; haz clic en ella o pulsa Ctrl+Alt+A para
desactivar Aprobar todo allí al instante. Solo tus teclas y clics responden a estos diálogos: las
herramientas del agente, incluida `type`, no pueden pulsar nada en ellos.

Los comandos se escriben en un shell que puedes observar, y no quedan en su historial. Los
comandos para esta máquina van a la consola propia de borg, que se muestra mientras se ejecutan
para que puedas responder a una petición como la de sudo; los comandos para un contenedor van al
shell de ese contenedor en la vista de Docker. El shell conserva su carpeta y sus variables entre
comandos.

El agente también puede escribir texto para que lo revises: un comando en el prompt, o un script
en vim o nano abiertos en la consola. No se ejecuta nada: nunca se pulsa Enter, y el texto de
varias líneas entra como un pegado, que los shells y los editores insertan sin ejecutarlo.

El agente también puede buscar en la memoria de un proceso igual que lo hace la búsqueda en
memoria: cadenas, indicadores, texto o bytes. La memoria puede contener contraseñas, claves y
mensajes privados, y todo lo que encuentra la búsqueda va al agente, así que la primera búsqueda
en cada proceso espera tu aprobación en un diálogo que indica el proceso, su programa y la
búsqueda. Una vez que lo apruebas, el agente puede volver a buscar en ese proceso y analizarlo
hasta que borg sale. Puede pedir leer como administrador, lo que abre la petición de contraseña
del propio sistema.

El agente también puede analizar con reglas YARA y comprobar la integridad del código, igual que
**Coincidencias de reglas** en el diálogo de Ctrl-F y **Y** e **I** en el mapa de memoria (consulta
[Analizar archivos con reglas](#scanning-files-with-rules), [Comprobación de integridad del
código](#code-integrity-check) y [Analizar la memoria con reglas](#scanning-memory-with-rules)). Un
archivo o una carpeta de la máquina donde se ejecuta borg se analiza sin preguntar, igual que una
lectura. El análisis de una carpeta lee todos los archivos que hay debajo en el mismo sistema de
archivos, incluidos los ocultos. Deja sin leer los archivos de más de 64 MiB y se detiene tras 20
000 archivos, 2000 coincidencias o 45 segundos. Los archivos de los contenedores no se analizan.

Con un proceso es distinto, porque el análisis lee su memoria. El primer análisis de cada proceso
espera tu aprobación en **El agente remoto quiere analizar la memoria de un proceso**, que indica el
proceso, su programa y las comprobaciones: la de integridad, las reglas o ambas. Una aprobación
cubre los análisis y las búsquedas en memoria de ese proceso hasta que borg sale, sea cual sea la
que pidió primero el agente. Cada comprobación se detiene tras 45 segundos, 1 GiB de memoria o 2000
hallazgos. El agente puede pedir leer como administrador, lo que abre la petición de contraseña del
propio sistema una sola vez para ambas comprobaciones.

Las reglas son las de Borg y las de tu carpeta de reglas, salvo que el agente aporte su propio texto
de reglas, de hasta 64 KiB. Así puede redactar una regla a partir de lo que encontró, probarla en
una carpeta o un proceso y afinarla. Ese texto se ejecuta solo, sin las reglas de Borg ni las tuyas,
no puede usar `include` y nunca se guarda. El texto que no compila vuelve al agente con el mensaje
del compilador, antes de cualquier diálogo. Para un proceso, el diálogo muestra el texto de reglas
del agente debajo de las comprobaciones, para que veas qué busca.

El agente recibe cada hallazgo, del más grave al menos grave, con su gravedad, su tipo, la regla y
su origen, el archivo y el desplazamiento o la dirección y la región de memoria, hasta 64 bytes de
lo que coincidió como texto o en hexadecimal, y qué más podría explicarlo. Después vienen cuánto se
leyó, qué no se pudo leer, si el análisis terminó y los archivos de reglas que quedaron fuera. Las
pruebas llegan al agente tal como se encontraron, claves y tokens incluidos, igual que con la
búsqueda en memoria.

El agente también puede capturar tráfico como lo hace **R** en la pantalla de red (consulta
[Capturar tráfico](#capturing-traffic)). Indica qué grabar: un puerto, una conexión, un proceso por
su PID o todo el tráfico de la interfaz. Para un proceso, Borg graba la interfaz entera y resume
las conversaciones de ese proceso. Una captura dura de 1 a 60 segundos, o 5 si el agente no indica
la duración. Cuando el agente no indica la interfaz, Borg mide durante un segundo y elige la que
elegiría el diálogo **Capturar tráfico**. Cada captura espera tu aprobación en un diálogo,
**El agente remoto quiere capturar tráfico**, que muestra el tráfico, la interfaz, la duración,
cuánto guardar de cada paquete y la carpeta. Mientras dura, el pie de pantalla la muestra, y al
terminar un aviso dice dónde la guardó Borg: en la carpeta del panel activo o en la que indique el
agente, legible solo por ti. La captura de un agente siempre se ejecuta como tú y nunca pide la
contraseña de un administrador, así que sin acceso termina con **No tienes permiso para capturar
en** y el nombre de la interfaz.

El agente recibe un resumen: los protocolos que Borg reconoció y luego cada proceso, host y
conexión con sus paquetes y bytes, con los nombres de host sacados de las propias respuestas DNS de
la captura y de los nombres de servidor TLS. Borg vigila los sockets mientras dura la captura, así
que las conversaciones quedan asignadas a sus procesos aunque la pantalla de red esté cerrada. El
resumen deja fuera lo que llevan los paquetes. Leer un archivo `.pcap`, `.pcapng` o `.cap` da el
mismo resumen, acotado con las palabras del filtro del visor de tráfico, y el agente puede pedir un
paquete por su número para obtener sus capas y sus bytes en un volcado hexadecimal. Borg recuerda
los procesos de las últimas 16 capturas del agente hasta que sale. Los demás archivos muestran sus
conversaciones en **Proceso desconocido**, salvo que sus paquetes lleven su proceso.

Mientras hay un agente conectado, **◈ Remoto** aparece al principio de la barra de atajos. Haz clic
en él, o elige F9 → Remoto → **Registro remoto…** en cualquier momento, para ver un registro de lo
que ha hecho el agente: lo que ha mirado, leído, analizado, mostrado y escrito, y cada comando y
cambio, aprobado o rechazado. Cada sesión guarda su registro en disco, legible solo por ti, así que
puedes volver a una sesión que dejaste en marcha y ver lo que pasó en ella, incluso después de que
Borg haya salido. Cuando el registro crece mucho, Borg descarta las entradas más antiguas. Antes de
guardar una entrada, Borg oculta las contraseñas, los tokens y las claves que reconoce, y deja fuera
la salida de los comandos. Una contraseña pasada como una palabra más no se distingue de las demás,
así que Borg no puede ocultarla. Para no guardar ningún registro, desactiva
**Remoto: llevar un registro de lo que hace** en Opciones → Complementos, lo que también borra el
registro guardado.

### Borg cloud

Borg cloud permite que Claude llegue a un borg al que no tiene ruta SSH: un servidor que abres en
un terminal del navegador, un contenedor o una máquina detrás de NAT. Ambos extremos se conectan
hacia fuera a un relay, y el relay los une.

1. En el borg del servidor, pulsa F1 y luego C. La primera opción es `borg pair` con un enlace;
   Enter lo copia. El enlace funciona una sola vez, durante diez minutos. Debajo está el relay:
   para usar otro, pega ahí su dirección y pulsa Enter para guardarla y obtener un enlace a
   través de él.
2. Ejecuta el comando en un terminal de tu computadora, donde también esté instalado borg. Muestra
   un código de emparejamiento: seis emojis, cada uno con su nombre, como
   `🐱 Cat · 🦁 Lion · 🐎 Horse`.
3. El borg del servidor pregunta si quieres emparejar, y muestra el nombre de tu computadora y el
   código. La pregunta aparece enseguida, incluso mientras F3 muestra el comando como texto. Elige
   Sí si los códigos coinciden. Después, `borg pair` ofrece añadir el servidor a
   Claude Code, como `claude mcp add borg-NAME -- borg mcp remote NAME`. Espera tu respuesta 5
   minutos; cuando se rinde o lo detienes, la pregunta se cierra y el enlace deja de funcionar.

El código procede del intercambio cifrado que los dos borgs acaban de completar, así que solo es
el mismo en ambas pantallas si es tu computadora la que está hablando con el servidor. Alguien que
se hubiera hecho con el enlace y lo usara antes mostraría al servidor un código distinto, y no
puede elegir uno que coincida con el tuyo. Si tu terminal dibuja los emojis más estrechos de lo
que borg espera, borg muestra solo los nombres; en ese caso, compara los nombres.

A partir de entonces, Claude puede llegar a ese borg siempre que esté en ejecución allí.
`borg pair` sin más argumentos enumera los servidores emparejados, y `borg pair --forget NAME`
olvida uno; en el servidor, F8 en el diálogo Acceso remoto olvida todas las computadoras
emparejadas. A través de borg cloud, Claude también obtiene `download` y `upload`, que copian
archivos entre el servidor y tu computadora; `borg get NAME:PATH [TO]` y
`borg put FROM NAME:PATH` hacen lo mismo desde tu terminal. Las subidas esperan tu aprobación en
borg, y cada aprobación indica qué computadora la pidió.

La conexión está cifrada de extremo a extremo. Cada borg guarda una clave de servidor, por la que lo
conocen las computadoras emparejadas, y una computadora que llega a servidores guarda además una
clave de computadora propia. Solo tú puedes leer ambas. La clave de servidor es un archivo en la
carpeta `link` de la carpeta de estado. La clave de computadora también es un archivo allí. En un
Mac se guarda en tu llavero de inicio de sesión, y en Linux el archivo se sella con una contraseña
(consulta [Claves de computadora](#computer-keys)). El emparejamiento intercambia las claves
públicas, y cada conexión empieza con un intercambio Noise IK ligado a ellas, de modo que el relay
solo reenvía texto cifrado y no puede responder en nombre de ninguno de los dos borgs. Las llamadas
desde otra computadora pueden leer sin preguntar, igual que las locales; activa "preguntar antes de
que otra computadora lea" en Opciones → Complementos para que esas lecturas también esperen
aprobación. Borg no mantiene ninguna conexión con el relay salvo que esté emparejado o mostrando un
enlace de emparejamiento, y ninguna en absoluto cuando borg cloud está desactivado en Opciones →
Complementos.

Borg guarda el relay en la configuración como `relay = "wss://relay.example.com"`. Una computadora
llega al servidor a través del relay con el que se emparejó, así que, si el servidor cambia de
relay, vuelve a emparejar la computadora. Todos los borgs de una misma máquina comparten sus claves,
así que solo el primero que se abre se conecta al relay; los demás te indican cuál tiene borg cloud.

#### Claves de computadora

La clave de computadora demuestra a cada servidor emparejado que una conexión viene de tu
computadora. Cuando un servidor todavía conoce esta computadora por otra clave, como la clave de
servidor de esta máquina, Borg lo cambia a la clave de computadora la próxima vez que abre una
sesión allí. Borg demuestra al servidor que tiene la clave nueva, y el servidor cambia la clave en
la entrada de esa computadora y conserva su nombre. Las conexiones que aún usan la clave anterior
terminan en su siguiente llamada. El servidor muestra un aviso y escribe ambas huellas digitales en
el registro del agente, y su lista de **Clientes** muestra cuándo cambió la clave y cuál era. Un
servidor con un Borg más antiguo conserva la clave anterior hasta que se actualice.

En un Mac, Borg guarda la clave de computadora en tu llavero de inicio de sesión, como un elemento
llamado **Borg Commander** que solo Borg puede leer sin preguntarte. La primera vez que un proceso
de Borg necesita la clave, macOS pide Touch ID o tu contraseña, así que incluso un programa que
inicie `borg` por su cuenta se deja ver. Después de cada actualización de Borg, macOS también
pregunta una vez si el Borg nuevo puede usar el elemento; elige **Permitir siempre**. Un archivo de
clave de computadora de un Borg anterior pasa al llavero la primera vez que Borg lo usa, y el
archivo se elimina. Un Borg que se ejecuta sin la pantalla del Mac, como por SSH o en el servicio en
segundo plano, no puede mostrar la petición ni conectarse a servidores. Cuando `$BORG_STATE_DIR`
indica una carpeta de estado distinta de la habitual, Borg guarda la clave de computadora como un
archivo en la carpeta `link` de esa carpeta. Las versiones de Borg para macOS se firman con Hardened
Runtime, así que otros programas no pueden leer la memoria de Borg ni cargar código en él.

En Linux, Borg sella la clave de computadora con una contraseña que tú eliges, en el archivo
`computer-key.sealed` de la carpeta `link`. Ejecuta `borg unlock` en una terminal para elegirla:
Borg sugiere cuatro palabras al azar, fáciles de escribir y difíciles de adivinar, y pide la
contraseña dos veces. Borg refuerza la contraseña con Argon2id, lo que lleva unos segundos y 1 GiB
de memoria, o la octava parte de la memoria en una máquina con menos de 8 GiB, así que adivinarla a
partir de una copia del archivo es lento. Un archivo de clave de computadora de un Borg anterior se
sella la primera vez que ejecutas `borg unlock`, y el archivo se elimina; hasta entonces, Borg te
pide que lo ejecutes. Cuando `$BORG_STATE_DIR` indica una carpeta de estado distinta de la habitual,
Borg también guarda en Linux la clave de computadora como un archivo normal, sin contraseña.

Una vez desbloqueada, la clave queda en el keyring del kernel de tu usuario hasta que pasan 12 horas
sin que Borg la use, y cada uso vuelve a contar las 12 horas desde el principio. Un reinicio la
bloquea, y `borg lock` la bloquea al instante. Donde Borg no puede usar el keyring del kernel, como
en muchos contenedores y servicios del sistema, un pequeño proceso de Borg guarda la clave, en un
socket de la carpeta `link` que solo tú puedes abrir, y termina tras 12 horas sin uso. Cuando la
clave está bloqueada, Borg pide la contraseña: los paneles abren el diálogo **Desbloquear clave**, y
`borg pair`, `borg get` y `borg put` la piden en la terminal. `borg mcp remote`, que Claude inicia
sin terminal, te indica que ejecutes `borg unlock`.

`borg unlock --ssh-agent` permite además que tu ssh-agent desbloquee la clave. Borg pide al agente
que firme un desafío fijo con una clave Ed25519, o con una clave RSA si no tiene ninguna, y sella la
clave una segunda vez con esa firma. A partir de entonces, Borg desbloquea la clave sin preguntar
mientras ese agente esté disponible, y la contraseña sigue funcionando. Borg comprueba que el agente
firma igual dos veces y rechaza una clave cuyas firmas cambian. Cualquiera que pueda usar tu agente
puede obtener la misma firma: si lo reenvías con `ssh -A`, root en la máquina remota puede usarlo
mientras estás conectado y, con una copia del archivo sellado, desbloquear esta clave. Reenvía el
agente solo a máquinas de confianza, o deja esto desactivado. `borg unlock --no-ssh-agent` lo vuelve
a desactivar.

`borg pair --new-key` da a esta computadora una clave nueva, y cada servidor la acepta la próxima
vez que Borg se conecta a él. Hasta que todos la tengan, Borg conserva también la clave anterior, y
`borg pair` marca cada servidor que todavía la acepta. Pasada una semana, también sugiere olvidar
los servidores que ya no uses, con `borg pair --forget NAME`.

Cuando un servidor rechaza todas las claves que tiene esta computadora, Borg te lo dice. O alguien
olvidó esta computadora en ese servidor, o alguien con una copia de su clave cambió antes el
servidor a una clave propia. Una clave nueva es higiene y no deshace una filtración. Si crees que
alguien copió una clave, pulsa F8 en Acceso remoto en cada servidor para olvidar las computadoras
emparejadas, vuelve a emparejar y revisa esos servidores.

### Sesiones con nombre y el servicio en segundo plano

Con espacios de trabajo con nombre, `borg sessions list` muestra sus ID y nombres,
`borg sessions create NAME` añade uno y `borg --session NAME` lo abre. Por MCP, el agente puede
llamar a `list_sessions`, `list_remote_sessions` y `use_session` para elegir con qué sesión trabaja
esa conexión. Una sola llamada a una herramienta también puede pasar un ID de `session` para
dirigirse a otra sesión solo esa vez. La sesión elegida solo se aplica a esa conexión MCP.

Con más de una sesión, o cuando la única que hay está abierta en otro terminal, `borg` empieza por
la ventana **Sesiones de Borg**. Sus primeras líneas explican qué es una sesión, y la lista muestra
cada sesión con la fecha en que se usó por última vez. Una sesión activa en este momento aparece
marcada con su estado: **Abierta en otro terminal**, o **Mantenida por el servicio** cuando la
ejecuta el servicio en segundo plano. Queda elegida la sesión usada por última vez, salvo que otro
terminal la tenga abierta. Enter abre la sesión elegida donde la dejaste; una sesión que mantiene el
servicio se abre conectada a él, como con `borg service attach NAME`. Un clic elige una sesión y un
segundo clic la abre. **F7** pone nombre a una sesión nueva y la abre, y Esc o F10 sale sin abrir
ninguna. Mientras la ventana está abierta, comprueba cada 2 segundos qué sesiones están activas.

**F8** o **Delete** elimina la sesión elegida, tras una confirmación cuya opción predeterminada es
**No**. Esto borra sus paneles guardados y su historial de terminal; tus archivos y carpetas se
conservan. La sesión predeterminada (Default) siempre se queda, igual que una sesión activa en este
momento: sal antes de Borg en el otro terminal, o ejecuta `borg service stop` para eliminar una
sesión que mantiene el servicio.

Cuando indicas con `borg --session NAME` una sesión que otro Borg ya tiene abierta y no hay ningún
servicio en segundo plano al que conectarse, `borg` se detiene e indica qué Borg es: su número de
proceso, su terminal tal como lo muestra `ps` y hace cuánto se inició. Sal de ese Borg con F10 donde
se esté ejecutando. Si ya no puedes llegar a su terminal, termínalo con `kill` y el número de
proceso.

`borg service install`, `enable` y `start` configuran el servicio opcional en segundo plano. Este
mantiene la conexión de emparejamiento de la máquina y deja las sesiones con nombre en marcha
después de que se cierren sus terminales. Estos comandos ejecutan el gestor de servicios del
sistema a través de sudo, que te pide la contraseña, o directamente cuando eres root.
`borg service attach NAME` vuelve a conectarse a una, y
Ctrl-] se desconecta de nuevo sin terminar su shell. Usa `borg service stop`, `disable` o
`uninstall` para detener o quitar el servicio. En un contenedor sin systemd, haz que el supervisor
de procesos del contenedor ejecute `borg service run`; los comandos `install` y `enable` necesitan
un host Linux con systemd.

Elige F9 → Remoto → **Servicio en segundo plano…** para configurar el servicio desde Borg. Cuando
el servicio no está instalado, un diálogo muestra el comando que lo instala, lo habilita y lo
inicia. Cuando está instalado y detenido, el diálogo ofrece habilitarlo e iniciarlo. Enter escribe
el comando en el shell de Borg y lo ejecuta allí, donde ves cómo sudo te pide la contraseña.
Cuando el servicio ya está en ejecución, o la máquina no tiene systemd, un aviso lo indica. Una vez
que el servicio funcione, sal de Borg con F10 y vuelve a ejecutar `borg` para abrir tu sesión en
el servicio.

### Paneles remotos

Después de emparejar, `borg --remote MACHINE:SESSION` abre los archivos de esa sesión en el panel
izquierdo. Enter o F3 previsualizan un archivo remoto descargando una copia local temporal. **F5**
copia archivos y carpetas marcados entre paneles locales y remotos, o entre dos hosts emparejados,
y el diálogo de destino indica qué cliente o host es cada lado. Las copias se ejecutan como tareas
en segundo plano con progreso en bytes, pausa y cancelación, y la opción de sobrescribir, omitir o
conservar ambos. Las carpetas se copian con sus subcarpetas vacías y sus enlaces simbólicos, y los
archivos normales conservan su permiso de ejecución y su fecha de modificación. Un archivo solo
aparece en su destino cuando ha llegado todo su contenido. F6 (mover) y F8 (eliminar) no están
disponibles en los paneles remotos.

El panel derecho sigue en tu computadora. Con **Opciones → Complementos → Seguir** activado, la
línea de comandos y **Ctrl-O** usan un shell interactivo en el host remoto. Su terminal se dibuja
localmente, y las teclas, lo que pegas, Ctrl-C y los programas de terminal se ejecutan
en el host remoto. Cada sesión remota conserva su propio shell y su línea de comandos a medio
escribir mientras cambias de panel, y al seleccionar un panel del cliente vuelve el shell local.

Los programas a pantalla completa, y los que leen la entrada del teclado en bruto como `top`,
reciben todas las teclas mientras se ejecutan, Ctrl-O incluida, y al cambiar el tamaño del
terminal cambia el del PTY remoto. Sal de un programa así con sus propias teclas (`q` en `top`).
Si lo iniciaste desde la línea de comandos del panel, Borg vuelve después a los paneles; si lo
iniciaste en la consola remota, la consola sigue abierta. Los paneles y el shell propios del Borg
remoto son independientes de esta consola. Para transferir archivos desde la línea de comandos,
usa `borg get MACHINE/SESSION:PATH [TO]` y `borg put FROM MACHINE/SESSION:PATH`.

Escribir `exit` en el shell principal Bash, Zsh o Fish de Borg pide confirmación. **No** está
seleccionado por defecto y mantiene en marcha el mismo shell, las variables, la carpeta y los
espejos. En una consola remota, `exit` pregunta si quieres cerrar el shell en el host; si eliges
Sí, vuelve a elegir la sesión con Alt-H para obtener uno nuevo.

Las carpetas remotas se cargan por partes, con un indicador de actividad y un recuento de las
entradas recibidas hasta el momento; al ir a otra carpeta se cancela la petición.

Mientras un panel remoto muestra una sesión, las solicitudes del agente que esperan allí aparecen
también en tu pantalla, y puedes responderlas aquí (consulta [Qué puede hacer el agente](#what-the-agent-can-do)).

### Hosts remotos

En el Borg local, pulsa **Alt-H**, haz clic en **Hosts remotos** en la barra de atajos o elige
**F9 → Remoto → Explorar hosts y sesiones…** para ver las máquinas emparejadas y sus sesiones con
nombre. Enter abre los archivos de la sesión seleccionada en el panel desde el que abriste el
selector, y sustituye lo que mostraba ese panel. Pulsa **M** para reflejar la pantalla de la sesión
y **R** para actualizar la lista.

**Tab** alterna entre **Hosts** (computadoras a las que este Borg puede llegar) y **Clientes**
(computadoras que pueden llegar a este Borg). Clientes enumera las computadoras emparejadas con este
Borg, las huellas de sus claves públicas, si están conectadas y cuántas conexiones tiene cada una.
Cuando una computadora cambió su clave, la línea de la huella digital también dice qué día y qué
huella tenía antes. Una computadora aparece una sola vez aunque un espejo use varias conexiones. El
estado se actualiza mientras el selector está abierto, también en una sesión cuyas conexiones
mantiene el servicio en segundo plano. El emparejamiento funciona en un solo sentido: un cliente
autorizado no se convierte automáticamente en un host cuyos archivos pueda explorar este Borg.
Cuando el propietario de la conexión no puede informar del estado de la conexión, los clientes
emparejados siguen apareciendo, con **Estado de conexión no disponible**. Un mismo Borg puede ser
host y cliente a la vez. El menú Remoto también ofrece **Reflejar host montado**, **Compartir este
Borg como host…**, **Registro remoto…** y **Servicio en segundo plano…**.

El selector muestra la versión de Borg de cada host. Al seleccionar una sesión se muestran su
sistema operativo y su arquitectura, si se ejecuta como servicio en segundo plano o en un terminal,
la latencia de las peticiones y la dirección del relay. Los hosts se consultan en paralelo, de modo
que un host no disponible no retrasa a los demás. Un host que no puede informar de su versión
muestra **versión no disponible**; la versión del relay es otra cosa. Mantén Borg actualizado
en ambas computadoras: un host al que le falta una función remota muestra un mensaje que te pide
actualizarlo.

Mientras un espejo se conecta, el selector muestra un estado animado, y **Esc** cancela al
instante. Cada paso de la conexión dispone de 10 segundos, y el intento completo de 30. Si el
intento falla, el selector sigue abierto e indica qué paso falló; **M** vuelve a intentarlo con la
sesión seleccionada.

En el selector, **F2** cambia el nombre del host seleccionado para todas sus sesiones, y
**Shift-F2** cambia el nombre de la sesión seleccionada. Estos nombres se guardan en este cliente y
aparecen en el selector, en los títulos de los paneles y en el pie de la consola. Deja un nombre
vacío para volver al original. Los detalles siguen mostrando los nombres originales del host y de
la sesión. Los nombres están ligados a la clave del host emparejado y al ID de la sesión; los
destinos de los comandos y los nombres en el host remoto no cambian.

**F8** sobre un host no disponible olvida su emparejamiento en este cliente, y **Shift-F8** olvida
el host seleccionado aunque esté en línea. La confirmación tiene No como opción predeterminada.
Olvidar un host cierra los paneles y las consolas de este cliente para ese host; las sesiones y los
archivos del host se conservan, igual que la identidad de este cliente y sus demás
emparejamientos. Vuelve a emparejar para reconectar.

**F8** o **Delete** sobre una sesión de un host la elimina, tras una confirmación cuya opción
predeterminada es **No**. Esto cierra la sesión y sus consolas remotas, y elimina sus paneles de
Borg guardados y su historial de terminal; los archivos y las carpetas del host se conservan. La
sesión predeterminada (Default) no se puede eliminar. Una sesión independiente que en ese momento
aloja la conexión no puede eliminarse a sí misma; ciérrala primero en el host y ejecuta Borg en
otra sesión. Si la eliminación falla, la sesión sigue en la lista para que puedas intentarlo de
nuevo. Las sesiones gestionadas por el servicio se detienen antes de eliminar su estado guardado,
y el servicio no las vuelve a crear.

### Reflejar una sesión

`borg mirror MACHINE:SESSION` muestra la pantalla en vivo de la sesión remota en tu terminal local;
Alt+Shift+M abre el mismo espejo desde un Borg local con un panel remoto. **Ctrl-G** (mantén
pulsado Control y pulsa G) sale del espejo y vuelve a los paneles locales, igual que un clic en el
banner de la parte inferior. La línea de encima indica quién tiene el control; Ctrl-] también
sale.

Un `borg` normal o un `borg --session NAME` se puede reflejar mientras se ejecuta, sin el servicio
en segundo plano. Su propio terminal sigue visible, y cualquier entrada de teclado o ratón en él
recupera el control del espejo. Mientras tienes el control, Borg en el host organiza la pantalla
para tu ventana: cuando el espacio sobre el banner es más pequeño que el terminal del host, Borg
dibuja en la esquina superior izquierda de ese terminal con tu tamaño, y el resto queda en blanco.
Cuando el control vuelve al terminal del host o sales del espejo, Borg vuelve a ocupar todo ese
terminal. Un espejo sin control sigue la disposición del terminal del host y se recorta para caber
en una ventana más pequeña; si agrandas la ventana, ves el resto. El servicio opcional
en segundo plano mantiene las sesiones vivas después de que se cierre su terminal original y
permite que el cliente que tiene el control cambie el tamaño de su PTY.

Si otro terminal tiene el control de la sesión remota, el espejo se abre en modo de solo lectura y
tus teclas no llegan a la sesión. Pulsa **Ctrl-T** o haz clic en la línea de estado para tomar el
control y desconectar el otro terminal, o empieza con `borg mirror MACHINE:SESSION --take`;
`--take` siempre toma el control de otro espectador. Una vez que tienes el control, la primera
tecla activa una pantalla remota en reposo, y las teclas y la entrada del ratón posteriores llegan
a ella. Un espectador que se queda atrás omite los fotogramas antiguos de
animación y pasa directamente a la pantalla actual.

Una solicitud del agente aparece en el espejo tal como la muestra el servidor, y la respondes allí
cuando tengas el control. Si abriste el espejo desde un panel remoto y la solicitud sigue esperando
cuando sales de él, el borg de ese panel vuelve a mostrarla.

### Ejecutar un relay

`borg-relay` es el relay a través del cual se encuentran los borgs. En un servidor Linux con un
dominio que apunte a él y el puerto 443 libre, un solo comando lo instala como servicio, con un
certificado TLS de Let's Encrypt:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install-relay.sh | sudo sh -s -- relay.example.com you@example.com
```

Ejecuta el mismo comando de nuevo para actualizarlo.

En Dokploy, Coolify u otro host de Docker cuyo proxy gestione TLS, ejecuta la imagen
`ghcr.io/0xcd21h/borg-relay:latest` y asígnale un dominio en el puerto 8080. En Dokploy, crea una
aplicación a partir de esa imagen de Docker con un dominio en el puerto 8080 del contenedor y HTTPS
activado; en Coolify, crea un recurso a partir de esa imagen de Docker con el puerto 8080 expuesto
y un dominio. Ambos aceptan también
[packaging/relay/docker-compose.yml](../../../packaging/relay/docker-compose.yml) como servicio de
Compose. La imagen ejecuta `borg-relay --behind-proxy`: el proxy sirve HTTPS y deja pasar los
WebSockets, y el relay aplica sus límites por dirección de cliente, según la indica el proxy en
`X-Forwarded-For`. Se ejecuta como un usuario sin privilegios y tiene una comprobación de estado
(`GET /health`).

En un host de Docker sin un proxy así, la misma imagen sirve TLS por sí misma:

```sh
docker run -d --restart unless-stopped -p 443:443 -v borg-relay:/data ghcr.io/0xcd21h/borg-relay --domain relay.example.com --cache /data
```

Detrás de otro proxy que gestione TLS, ejecuta `borg-relay --behind-proxy --listen 127.0.0.1:8080`.
Ejecútalo así solo detrás de un proxy, porque el relay confía en la dirección de cliente que indica
el proxy. El relay impone límites por dirección en servidores y sesiones, tamaño de trama, ancho de
banda, bytes por sesión, tiempo de inactividad y duración de la sesión. No guarda nada salvo su
certificado.

El relay solo registra recuentos: una vez por minuto, si algo ha cambiado, escribe una línea como

```text
stats: connections=3 servers=2 sessions=1 joined=14 carried=3.2MiB limited=0
```

Esto muestra las conexiones que mantiene, los servidores registrados y las sesiones abiertas, y,
desde que se inició, las sesiones unidas, los bytes que transportaron y las conexiones y sesiones
que un límite rechazó o terminó. Para depurar, `--log`, o la variable de entorno
`BORG_RELAY_LOG=1` en la imagen, registra además el registro de cada servidor por el inicio de su
clave, cada sesión con los bytes que transportó y cada límite alcanzado con la dirección del
cliente.

## Búsqueda, tareas y visores

Ctrl-S abre la búsqueda rápida. Escribe un nombre o un patrón glob como `*.rs`; Down y Up se mueven
entre las coincidencias, Insert marca una coincidencia, Ctrl-A las marca todas, y Enter o Esc
cierran la búsqueda. No se distinguen mayúsculas y minúsculas salvo que la consulta contenga
mayúsculas.

### Copiar, mover y eliminar

F5, F6 y F8 inician tareas de copia, movimiento y eliminación. Alt-J abre la lista de tareas,
donde puedes pausarlas o cancelarlas.

El diálogo Copiar tiene una casilla **Optimizar la copia para discos duros**. Pulsa Tab para
llegar a ella y Space para marcarla o desmarcarla, o haz clic en ella. Está desmarcada en cada
copia nueva. En este modo, Borg lee los archivos pequeños en memoria por lotes y luego escribe cada
lote, lo que puede reducir los desplazamientos del cabezal al copiar dentro de un mismo disco duro.
Un lote contiene hasta 64 MiB de datos o 4096 archivos; los archivos de más de 1 MiB se copian de
la forma habitual. Cuánto ayuda depende del disco, del sistema de archivos y de la caché del
sistema operativo, y puede ser más lento al copiar entre discos distintos.
Cuando ya existe un archivo con el mismo nombre: `o` sobrescribe, `s` omite, `k` conserva ambos,
`a` sobrescribe todos, `n` omite todos y Esc cancela. Las copias se escriben en archivos temporales
y se renombran cuando están completas.

### Tareas largas y discos

Borg sabe en qué tipo de disco está cada carpeta: un disco duro, un SSD, un disco en RAM, un recurso
compartido de red como NFS, SMB o sshfs, o la máquina que muestra un panel remoto. En macOS consulta
a `diskutil` y `hdiutil`, y en Linux lee `/proc/self/mountinfo` y los indicadores del dispositivo en
`/sys`. Ctrl-L muestra el disco y su sistema de archivos en la fila **Disco**, y la fila
**Velocidad** dice a qué velocidad espera Borg que lea y escriba.

Antes de que una tarea escriba nada, Borg calcula cuánto tardará a partir de los bytes que lee y
escribe en cada disco y del tiempo que lleva comprimir. Cuando una copia, un movimiento a otro disco
o una descompresión tardaría más de 2 minutos, o un cambio en un archivo comprimido más de 20
segundos, el diálogo **Tarea larga** dice cuánto tardará aproximadamente y en qué tipo de disco, y
espera:

- **Empezar** (`s`) ejecuta la tarea.
- **Siempre para este archivo** (`f`) la ejecuta y deja de preguntar por este archivo comprimido.
  La descompresión y los cambios en un archivo comprimido ofrecen esta opción.
- **Siempre en este tipo de disco** (`d`) la ejecuta y deja de preguntar por el trabajo en discos de
  este tipo.
- **Cancelar** (`c`) se detiene antes de la primera escritura.

Al guardar desde el editor en un archivo comprimido, la pregunta aparece encima del editor, que
recupera las teclas en cuanto respondes.

Mientras Borg no haya medido un disco, cuenta con la velocidad habitual de su tipo: unos 120 MB/s
para un disco duro, 900 MB/s para un SSD, 4 GB/s para un disco en RAM, 60 MB/s para un recurso
compartido de red y 10 MB/s para una máquina remota. Cada tarea que dura al menos 2 segundos y mueve
al menos 64 MiB le enseña a Borg la velocidad real del disco, y entonces la fila **Disco** dice
**velocidad medida**. Una tarea que pasa su tiempo comprimiendo no le enseña nada sobre el disco.

Borg guarda las velocidades y tus respuestas en `drives.json`, en su carpeta de estado, con hasta 256
archivos comprimidos. Solo tú puedes leer el archivo, porque nombra los archivos comprimidos sobre los
que respondiste. Todos los Borg que tengas abiertos lo comparten: cada uno lo lee al arrancar y otra
vez justo antes de escribir, así que lo que un Borg aprendió o le dijiste se mantiene cuando otro
escribe. La fila **Tareas largas** de Ctrl-L dice si Borg pregunta primero en ese disco.
**F9 → Opciones → Volver a preguntar antes de tareas largas** olvida todas las respuestas Siempre,
también las dadas en otros Borg. Un Borg que ya esté abierto conserva las respuestas que tiene hasta
que vuelva a escribir el archivo o lo inicies de nuevo.

### Visor y vista rápida

F3 abre el visor. Con el complemento Sistema activado, F3 sobre un archivo `.pcap`, `.pcapng` o
`.cap` abre en su lugar el visor de tráfico de la pantalla de red (consulta
[Capturar tráfico](#capturing-traffic)). En los archivos de texto, las flechas y PgUp/PgDn
desplazan, `w` activa o desactiva el ajuste de línea, y Esc o `q` lo cierran. En las imágenes,
Left/Right o `p`/`n` muestran la imagen anterior o la siguiente. Borg usa el protocolo de imágenes
del terminal cuando lo hay; los demás terminales muestran la imagen como bloques de colores.

Ctrl-Q muestra una vista previa rápida en el otro panel. Tab entra en ella, donde las flechas,
PgUp/PgDn, Home/End y la rueda del ratón desplazan; Tab o Esc vuelven a la lista de archivos. Ctrl-L
muestra la información del archivo. F3 abre una vista previa de texto en el visor completo, y al
cerrar el visor se vuelve a la vista previa conservando la posición, el ajuste de línea, el modo de
código fuente y las filas hexadecimales. Ambos leen los archivos de la misma forma limitada, así que
abrir un archivo de texto grande solo lee aproximadamente la primera página y es rápido sea cual sea
su tamaño. En un resultado de una búsqueda de texto, la vista previa se abre en cambio en la primera
coincidencia del archivo, con la consulta marcada como la marca la barra de búsqueda, y solo lee
alrededor de esa coincidencia.

Los dos visores de texto resaltan los lenguajes de programación, de marcado y los archivos de
configuración más comunes, entre ellos Rust, Python, JavaScript, TypeScript/TSX, Go, C/C++, Java,
C#, Ruby, PHP, Lua, SQL, HTML/CSS, JSON, YAML, TOML, scripts de shell y Dockerfiles. Markdown se
abre con formato por defecto; pulsa `s` o F4 en cualquiera de los dos visores para ver el código
fuente. Se admiten encabezados, énfasis, listas, listas de tareas, enlaces, tablas y bloques de
código delimitados. Los archivos Markdown de más de 2 MiB se abren como código fuente para limitar
el uso de memoria.

JSON se abre organizado con un valor por línea, con las claves en su orden original y los números
exactamente como están escritos, lo que ayuda sobre todo con archivos escritos en una sola línea.
Pulsa `s` o F4 para ver el archivo tal como está escrito. Los archivos JSON Lines (`.jsonl`,
`.ndjson`) se organizan registro a registro. El JSON de más de 16 MiB, o que no se puede analizar,
se abre tal como está escrito.

Pulsa `h` en el visor de texto o en la vista previa para ver los bytes del archivo en filas
hexadecimales y vuelve a pulsarla para volver al texto. Un archivo binario, que Borg reconoce por un
byte a cero en sus primeros 4 KiB, se abre en filas hexadecimales en ambos, y `h` muestra su texto.
Cada fila empieza con su posición en el archivo y luego muestra sus bytes en hexadecimal y los
mismos bytes como texto, con un punto en cada byte que no se puede imprimir. Las posiciones y los
bytes a cero se ven atenuados. Una fila tiene 16 bytes, o 8, 4, 2 o 1 cuando no hay espacio para 16.
Borg lee solo las filas que hay en pantalla, así que las flechas, PgUp/PgDn, Space, Home y End
recorren al instante un archivo de varios gigabytes. La barra de búsqueda también funciona en las
filas hexadecimales y marca cada coincidencia en las dos columnas. En hexadecimal, los archivos
Markdown y JSON muestran los bytes tal como están escritos.

En el visor, una franja de entropía recorre el borde derecho de las filas hexadecimales. La entropía
indica lo variados que son los bytes, desde 0 bits por byte para un solo byte repetido hasta 8 para
bytes tan aleatorios como los datos comprimidos, cifrados o empaquetados. El texto suele rondar 4,5,
y el código, entre 5 y 6,5. La franja cubre todo el archivo de arriba abajo. La barra de cada celda
es tan larga como la entropía más alta de su parte del archivo, atenuada por debajo de 4 bits por
byte y en rojo y negrita desde 7,2, y las celdas de los bytes que hay en pantalla se marcan como en
una barra de desplazamiento. La línea de estado muestra la entropía en la página y si es **baja**,
**típica** o **alta**. Pulsa `]` y `[` para saltar a la siguiente o a la anterior serie de bloques
de entropía alta, o haz clic en la franja para ir a esa parte del archivo.

Borg lee la entropía en segundo plano, en bloques de 4 KiB. Un archivo de hasta 256 MiB se lee
entero, y uno mayor se muestrea en 65 536 bloques repartidos por igual, así que la memoria que usa
es la misma sea cual sea su tamaño. Una entropía alta donde esperarías código, cabeceras o tablas
puede indicar datos empaquetados o cifrados escondidos ahí. La multimedia comprimida, los archivos
comprimidos, los volúmenes cifrados y los almacenes de claves tienen entropía alta de principio a
fin, así que interpreta la franja según lo que debería contener el archivo.

### Archivos comprimidos

Pulsa Enter sobre un archivo comprimido para abrirlo como una carpeta. Borg abre los archivos cuyo
nombre termina en `.zip`, `.7z`, `.rar`, `.tar`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tbz2`, `.tbz`,
`.tar.xz`, `.txz`, `.tar.zst` o `.tzst`, y los archivos comprimidos sueltos que terminan en `.gz`,
`.bz2`, `.xz` o `.zst`, que se abren como una carpeta con ese único archivo. Ctrl-PgDn también abre
un archivo comprimido, y F3 sobre el propio archivo comprimido sigue mostrando sus bytes en el
visor. El título del panel muestra una marca como **▤ zip** con el formato que Borg encontró en los
primeros bytes del archivo, y la ruta continúa dentro del archivo comprimido. Las carpetas de dentro
muestran el tamaño de lo que contienen. Left, Enter en `..` o Ctrl-PgUp en la raíz del archivo
comprimido te devuelven a la carpeta que lo contiene, con el cursor sobre él. Si el archivo
comprimido se borró o ya no se puede leer, subir desde cualquiera de sus carpetas también te
devuelve allí, y el aviso dice por qué.

Para listar un archivo comprimido, Borg lee lo que contiene, lo que en un tar comprimido significa
leer el archivo entero. Mientras lee uno grande, el pie del panel muestra hasta dónde ha llegado
Borg, y Esc detiene la lectura y vuelve atrás. Borg recuerda los listados de los últimos 4 archivos
comprimidos mientras conserven el mismo tamaño y la misma fecha, así que entrar y salir de nuevo es
instantáneo. Los archivos comprimidos con más de 1.000.000 de entradas no se abren.

F3 o Enter sobre un archivo de dentro lo descomprime en una carpeta privada dentro de la carpeta de
ejecución de Borg y lo abre en el visor. La descompresión se ejecuta como una tarea, así que un
archivo grande muestra su progreso, y Esc la cancela. Borg borra el archivo descomprimido en cuanto
el visor lo tiene abierto.

F4 sobre un archivo de dentro lo abre en el editor, y Alt-E lo abre en el editor del panel. Borg
descomprime el archivo en la misma carpeta privada, y el título del editor muestra su ruta dentro del
archivo comprimido. Cada vez que guardas, el archivo vuelve a escribirse en el archivo comprimido
como una tarea, y el editor lo avisa ("Guardado. Escribiéndolo en site.zip."). Si el archivo
comprimido no lo acepta, el aviso dice por qué y dónde queda tu edición.

F5 copia los archivos y carpetas marcados fuera del archivo comprimido, a la carpeta del otro panel
o a la carpeta que escribas en el diálogo Copiar. **Extraer aquí** y **Extraer en el otro panel**,
en el menú Archivo (F2), descomprimen el archivo comprimido entero bajo el cursor en la carpeta de
este panel o en la del otro. En ambos casos los archivos llegan a una carpeta de esta máquina, y la
tarea aparece en Alt-J con su progreso, su velocidad y el tiempo restante. Pregunta por los nombres
que ya existen igual que una copia, escribe cada archivo con un nombre temporal hasta que está
completo y detiene cada archivo en el tamaño que le asigna el archivo comprimido.

Borg modifica archivos zip, 7z y tar, con compresión o sin ella. F5 o F6 desde una carpeta de esta
máquina copia o mueve los archivos marcados al archivo comprimido que muestra el otro panel, o a la
carpeta de él que escribas en el diálogo. F7 crea una carpeta en el archivo comprimido, y F8 borra
las entradas marcadas después de preguntar. F6 hacia un nombre del mismo archivo comprimido cambia
el nombre de las entradas o las mueve allí, y F6 hacia una carpeta de esta máquina las saca: se
descomprimen allí y luego salen del archivo comprimido. Cuando un nombre ya existe en el archivo
comprimido, Borg pregunta igual que una copia, y también pregunta cuando dos de los archivos que
copias comparten nombre, como puede pasar con archivos de resultados de búsqueda o de una colección.
Un cambio de nombre a un nombre que el archivo comprimido ya tiene se detiene con un aviso. Un
movimiento solo borra lo que llegó entero, así que un archivo que omites se queda donde estaba,
igual que uno que reemplazó otro archivo posterior con el mismo nombre.

Cada cambio se ejecuta como una tarea en Alt-J. Borg escribe el archivo comprimido modificado junto
al original con un nombre temporal, lo vuelve a leer y solo entonces lo pone en el lugar del
original con los mismos permisos. La relectura comprueba la lista de entradas de un zip o un 7z y
cada entrada de un tar. Un archivo entra tal como está cuando Borg lo escribe, así que un archivo
que vuelves a guardar mientras la tarea espera tu respuesta entra con su contenido más reciente. Un
cambio que falla o que cancelas deja el archivo comprimido como estaba. Las entradas que el cambio
conserva se copian tal como están guardadas: un zip copia sus datos comprimidos, y un 7z copia cada
bloque sólido que conserva todas sus entradas y vuelve a comprimir solo los bloques que pierden
alguna. Un tar comprimido se descomprime y se vuelve a comprimir entero, y se descomprime una vez
más para releerlo, así que cambiar un `.tar.xz` grande tarda casi lo mismo que crearlo y
descomprimirlo. Un zip o un tar sin comprimir que solo gana entradas se copia y las entradas nuevas
van detrás de la última, y en APFS, Btrfs, XFS y bcachefs esa copia es instantánea. Antes de un
cambio que tardaría mucho, Borg pregunta primero (consulta [Tareas largas y
discos](#long-jobs-and-drives)).

En un archivo zip o 7z cifrado, las entradas nuevas se cifran con AES-256 y con la contraseña con la
que abriste el archivo comprimido, y un 7z que oculta su lista de entradas la sigue ocultando. Las
entradas que el cambio conserva quedan como estaban.

Los archivos RAR y los archivos comprimidos sueltos como `.gz` siguen siendo de solo lectura, y al
intentar cambiarlos Borg lo avisa. Ctrl-Q y **Comprimir…** no funcionan dentro de un archivo
comprimido. Un archivo comprimido dentro de otro se abre cuando lo copias fuera. El shell se queda en la carpeta que contiene el archivo comprimido, y
una sesión guardada vuelve a abrir esa carpeta con el cursor sobre él.

Los archivos zip y 7z cifrados piden la contraseña al abrirse. En el diálogo **Contraseña**, cada
carácter que escribes aparece como ruido de interferencia que cambia sin parar y destella con cada
tecla. El ruido depende solo de cuántos caracteres has escrito. El diálogo vuelve a preguntar si la
contraseña es incorrecta, y Esc sale del archivo comprimido. Borg guarda la contraseña en memoria mientras el panel muestra el archivo
comprimido y nunca la escribe en ningún sitio. **Extraer aquí** no puede pedirla, así que para un
archivo comprimido cifrado pulsa Enter sobre él, escribe la contraseña y copia los archivos fuera
con F5.

Antes de escribir nada, Borg suma lo que ocupan las entradas elegidas una vez descomprimidas y
pregunta primero cuando pasan de 1 GiB y de 100 veces el tamaño del archivo comprimido, o de 100.000
archivos, que es el aspecto de una bomba zip. También pregunta cuando el destino tiene menos espacio
libre del que necesitan los archivos, y un cambio en un archivo comprimido pregunta cuando su disco no
tiene sitio para el nuevo archivo comprimido junto al antiguo. **Descomprimir de todos modos** (`u`) sigue adelante, y
Cancelar se detiene antes del primer archivo. Las entradas con rutas absolutas, con `..` en la ruta
o que son archivos de dispositivo se omiten con un aviso, y ninguna entrada ni enlace de un archivo
comprimido puede hacer que Borg escriba fuera de la carpeta de destino.

Borg lee por sí mismo zip, 7z y la familia tar. Los archivos RAR pasan por 7-Zip (`7zz` o `7z`) o
por `bsdtar`, el que Borg encuentre primero, y macOS ya trae `bsdtar`. Si no hay ninguno instalado,
Enter sobre un archivo RAR ofrece instalar bsdtar. El diálogo **Instalar bsdtar** funciona como el
de tcpdump en [Capturar tráfico](#capturing-traffic): muestra el comando exacto para tu gestor de
paquetes, y Enter lo escribe en el shell de Borg. Borg también busca `bsdtar` en la carpeta
`libarchive` de Homebrew, que Homebrew deja fuera del `PATH`. Los archivos RAR cifrados todavía no
se abren.

### Editor

F4 abre el archivo seleccionado en el editor de Borg, y Shift-F4 lo abre en `$VISUAL` o `$EDITOR`.
El editor conserva los finales de línea y la sangría del archivo, resalta el código como los
visores y muestra los números de línea. Shift con las flechas selecciona texto; Ctrl-C, Ctrl-X y
Ctrl-V copian, cortan y pegan mediante el portapapeles de Borg, y el texto pegado desde el
terminal entra exactamente tal cual. Ctrl-Z y Ctrl-Y deshacen y rehacen, y Tab y Shift-Tab
aumentan y reducen la sangría. F2 o Ctrl-S guardan a través de un archivo temporal, de modo que un
archivo nunca queda escrito a medias. Los archivos que no son texto UTF-8, o que superan los 32
MiB, se abren en el visor. Las líneas de más de 20.000 caracteres, como el código minificado, no
se resaltan, y solo se dibuja la parte de la línea que aparece en pantalla, así que incluso un
archivo grande escrito en una sola línea se edita sin retrasos. Alt-Shift-F organiza un archivo
JSON con un valor por línea, usando la sangría del propio archivo, en un solo paso que Ctrl-Z
deshace; el JSON que no se puede analizar se queda como está, y la línea de estado indica dónde se
detuvo el análisis.

El editor hace sus preguntas en diálogos sobre el texto: Tab se mueve entre campos y botones,
Enter confirma el seleccionado, Esc cancela, y puedes hacer clic en cualquiera de ellos. Ctrl-F o
F7 buscan texto con la barra de búsqueda que se describe más abajo. Ctrl-R o F4 reemplazan, con
las mismas opciones que la barra de búsqueda: Enter reemplaza la coincidencia y pasa a la
siguiente, **Saltar** la deja como está, y Alt-A o **Todo** reemplaza todas las coincidencias en
un solo paso que se deshace con un único Ctrl-Z. Con una expresión regular, `$1` en el reemplazo
vuelve a poner lo que encontró su primer grupo. Ctrl-G va a una línea. Esc cierra el editor, y
antes pregunta si quieres guardar o descartar los cambios sin guardar; si el archivo cambió en el
disco desde que lo abriste, Borg pregunta si quieres sobrescribirlo o volver a cargarlo. Un clic
coloca el cursor, arrastrar o hacer Shift-clic selecciona, un doble clic selecciona una palabra y
la rueda desplaza; también puedes hacer clic en las teclas que aparecen en la línea de estado.

Alt-E abre el archivo seleccionado en el editor de Borg dentro del panel activo, en lugar de sus
archivos, para que puedas editar dos archivos uno al lado del otro, o un archivo junto a la carpeta
en la que está. En los resultados de búsqueda y en las colecciones, Alt-E abre el editor en el otro
panel, para que la lista siga a la vista. Alt-E sobre otro resultado lo abre en lugar del archivo
que hay ahí. Si ese archivo tiene cambios sin guardar, Alt-E trae su editor al frente y pregunta si
quieres guardarlos. F6 pasa del editor de un panel al otro panel, y Tab vuelve al editor desde la
lista de archivos. El texto copiado con Ctrl-C en un editor se pega con Ctrl-V en el otro. Mientras
un editor tiene el foco, la barra de atajos muestra las teclas del editor, y las teclas del panel
vuelven con la lista de archivos. Ctrl-O también muestra la consola desde un editor, y F9 abre los
menús. Esc o F10 cierra el editor y devuelve al panel sus archivos; al salir, Borg pregunta primero
por cada editor de panel con cambios sin guardar. La vista rápida y la información del archivo
necesitan el otro panel, así que esperan hasta que se cierre su editor. Un editor abierto junto a
los resultados les devuelve el foco al cerrarse y trae de vuelta la vista rápida o la información
del archivo que mostraban.

El visor y el editor buscan texto de la misma manera. En el visor, `/`, F7 o Ctrl-F abren una
barra de búsqueda en la línea de estado; en el editor, Ctrl-F o F7. Borg busca mientras escribes,
a partir del punto donde se abrió la barra: todas las coincidencias en pantalla quedan marcadas,
la actual destaca, y la barra las cuenta (3 de 17). Enter, Down o F3 van a la siguiente
coincidencia, y Shift-Enter, Up o Shift-F3 a la anterior; después de la última, la búsqueda sigue
desde el otro extremo y lo indica. Alt-C distingue mayúsculas y minúsculas (si no, una búsqueda en
minúsculas encuentra cualquier combinación), Alt-W busca palabras completas y Alt-R interpreta la
búsqueda como una expresión regular; también puedes hacer clic en las opciones. Ctrl-P y Ctrl-N
recuperan las búsquedas recientes. Esc cierra la barra; después, `n` y `N` en el visor, o F3 y
Shift-F3 en el editor, recorren las coincidencias. En un archivo grande, el visor busca en segundo
plano, muestra hasta dónde ha llegado y se detiene al pulsar Esc. Cuenta las coincidencias en
archivos de hasta 64 MiB, hasta 10.000 de ellas.

## Configuración

Cambia el idioma de la interfaz en **F9 → Opciones → Idioma**. Elige inglés, chino simplificado,
portugués (Portugal), español o ruso y pulsa Enter; Esc cancela. La elección se guarda en
`config.toml` como `language = "en"`, `"zh-CN"`, `"pt"`, `"es"` o `"ru"`. `BORG_LANGUAGE` tiene
prioridad sobre el ajuste guardado. Si no se ha definido ninguno de los dos, Borg sigue `LC_ALL`,
`LC_MESSAGES` o `LANG`, y usa el inglés para los idiomas que no admite. Los comandos, las rutas,
los nombres de archivo y los atajos de teclado nunca se traducen. El menú de idioma de este sitio
web cambia el idioma de la guía por separado.

Los ajustes se guardan en `~/.config/borg-commander/config.toml`, o en la carpeta indicada por
`$XDG_CONFIG_HOME` o `$BORG_CONFIG_DIR`. F9 → Opciones → Configuración los muestra como casillas;
Space o un clic activa o desactiva un ajuste y lo guarda al instante. Esc cierra la ventana.

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

En un terminal local con color verdadero, Borg arranca con una breve animación de su logotipo con
efecto glitch; cualquier tecla o clic la omite. Por SSH, y en terminales sin color verdadero, se
abre directamente en los paneles.

Tras `screensaver-after` segundos sin actividad, Borg muestra uno de cinco salvapantallas, elegido
al azar: un cubo que gira en un campo de estrellas, el logotipo con efecto glitch, un reloj con las
letras de bloques del logotipo que se desgarra con cada nuevo minuto, el logotipo que gira en
espiral hasta formar una galaxia y vuelve, o el logotipo reconstruido tabla a tabla mientras las
tablas viejas forman un segundo logotipo debajo. Cualquier tecla o clic devuelve los paneles, y
esa tecla no hace nada más. La aprobación que pide un agente termina el salvapantallas por sí sola,
así que nunca espera detrás de él.

Los temas personalizados van en `~/.config/borg-commander/themes/`. Un tema puede extender uno de
los integrados:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

### Archivos de Borg

Las carpetas de los paneles, las posiciones del cursor, la división, los modos de listado y la
ordenación se guardan en la carpeta de estado, `~/.local/state/borg-commander`, o en la carpeta
indicada por `$XDG_STATE_HOME` o `$BORG_STATE_DIR`.

Mientras se ejecuta, Borg guarda sus scripts de integración con el shell y los archivos copiados
desde contenedores en su carpeta de ejecución, `$XDG_RUNTIME_DIR/borg-commander`, o en
`borg-commander-<uid>` dentro de la carpeta temporal. Solo tú puedes abrir esta carpeta; si ya
existe una creada por otra persona, o en la que pueden entrar otros usuarios, Borg la ignora y usa
otro nombre. Su archivo `TTL` empieza con la hora hasta la que la carpeta está en uso: cada borg en
ejecución adelanta esa hora un día cada hora, así que, una vez pasada, la carpeta se puede eliminar.

**F9 → Opciones → Archivos de Borg…** muestra cada lugar donde Borg guarda archivos, cuánto ocupa
cada uno y dónde está:

- **Archivos temporales**: scripts de integración con el shell, peticiones de cambio de carpeta
  desde el shell, copias desde contenedores, vistas previas remotas, archivos descomprimidos de
  archivos comprimidos para el visor y el editor, y los certificados del proxy de una captura, en la
  carpeta de ejecución.
- **Sockets**: por donde los agentes y los espejos llegan a las sesiones en curso, en
  `/tmp/borg-<uid>-default`, o en una carpeta `/tmp/borg-<uid>-` con el nombre de la carpeta de
  ejecución cuando `$XDG_RUNTIME_DIR` está definida.
- **Cachés**: tamaños de carpeta e índices de nombres, en la carpeta de caché (consulta
  [Análisis de tamaño guardados](#saved-size-scans)).
- **Sesiones e historial**: sesiones, salida del shell, registro remoto, historial de búsquedas y
  carpetas, colecciones, equipos emparejados con sus claves y lo que Borg aprendió sobre tus discos
  (`drives.json`), en la carpeta de estado.
- **Configuración**: `config.toml` y tus temas.
- **Capturas**: las capturas de tráfico que guardó Borg, estén donde estén.

Up y Down eligen una fila, y una nota breve bajo la lista dice qué guarda el lugar elegido y cuánto
tiempo se conserva. Enter abre la carpeta de ese lugar en el panel activo. Esc cierra la ventana.

Borg limpia por su cuenta 30 segundos después de arrancar y luego cada hora. Solo elimina nombres
que crea él mismo, y nunca nada que haya cambiado en los últimos 10 minutos, así que un Borg que
está arrancando conserva lo que acaba de crear. Esto es lo que se elimina:

- Carpetas de otras versiones de Borg en la carpeta de ejecución, cuando ya ningún Borg se ejecuta
  desde ellas y, además, ningún Borg estaba en marcha cuando arrancó este o nada en ellas ha
  cambiado en 3 días.
- Peticiones de cambio de carpeta, archivos a medio escribir, carpetas de consolas remotas y
  certificados del proxy de capturas de procesos de Borg que terminaron.
- Copias desde contenedores, vistas previas remotas y archivos descomprimidos para el visor y
  el editor de más de un día.
- Sockets en los que nadie responde, y carpetas de sockets en las que ya no responde nada.
- Análisis de tamaño en el formato de Borg 0.4 y 0.5, análisis que la caché ya no lista e índices de
  nombres de carpetas que ya no se indexan.
- Carpetas de ejecución que versiones antiguas de Borg dejaron abiertas a otros usuarios, cuando
  nada en ellas ha cambiado en 3 días.

Las sesiones, el historial, la configuración y las capturas se conservan hasta que los elimines. La
línea bajo la nota dice cuánto sobra ahora mismo y qué eliminó la última limpieza. F8 elimina los
restos tras preguntar, y la pregunta indica las carpetas de las que vienen.

Cada captura que guarda Borg entra en una lista de las 1000 más recientes. La ventana muestra las
que siguen en el disco bajo **Capturas guardadas por Borg**, con su tamaño y fecha, y **con claves**
cuando junto a una hay un archivo `.keylog` con el mismo nombre. Enter sobre una captura abre su
carpeta con el cursor en el archivo, y ahí F8 la elimina. Las capturas guardadas antes de que Borg
empezara a llevar esta lista no aparecen en ella.

`borg storage` muestra los mismos lugares en el terminal y dice lo que sobra, y `borg storage clean`
lo elimina.

Para compilar, probar o contribuir, consulta [Desarrollo](../../../DEVELOPMENT.md).

## Una pequeña distracción

Abre Acerca de con F1 y escribe `2048` para jugar en una ventana. Las flechas, o un clic hacia un
lado del tablero, mueven las fichas; U deshace el último movimiento, R empieza una partida nueva y
Esc la cierra. Cuando llegues a 2048, pulsa Enter para seguir jugando. El tablero y tu mejor
puntuación se conservan entre sesiones.

## Actividad y memoria

**Alt-M** o **Comando → Monitor de actividad** abre el monitor de procesos. Lo proporciona el
complemento Sistema incluido, que está activado por defecto y se puede desactivar en Opciones →
Complementos. La lista de procesos ocupa el panel activo, y el otro panel muestra un resumen de la
memoria del proceso seleccionado. **Enter** o un doble clic abre ese proceso en una ventana sobre
el monitor, con su mapa de memoria a un lado y una vista previa de los bytes de la región
seleccionada al otro. **Esc** la cierra, y **Tab** alterna entre los dos lados. **Alt-W** o el
botón **[+]** maximiza el panel con el foco, **[=]** restaura la división y **[-]** oculta el
monitor. El monitor toma una muestra por segundo mientras está abierto y se detiene al cerrarlo.
El muestreo se pausa mientras se ejecuta una inspección de memoria y se reanuda cuando llega el
resultado.

La lista muestra la CPU, la memoria residente, el crecimiento de la memoria y las velocidades de
lectura y escritura en disco. Una CPU al 100 % equivale a un núcleo totalmente ocupado. Las páginas
residentes compartidas entre procesos cuentan para cada uno de ellos, así que sumar la RAM de todos
los procesos no da el total del sistema. Las velocidades de CPU y de disco aparecen a partir de la
segunda muestra.

| Tecla | Acción |
| --- | --- |
| Enter | Abrir el mapa de memoria del proceso / pasar a la vista previa de la región |
| `/` | Filtrar por nombre o PID; ↑↓ siguen moviendo, Enter conserva el filtro, Esc lo borra |
| C / M / G / N / P | Ordenar por CPU / RAM / crecimiento / nombre / PID; pulsa de nuevo para invertir |
| I / O | Ordenar por velocidad de lectura / escritura |
| T | Mostrar u ocultar el árbol de procesos |
| Left / Right | En el árbol, plegar o desplegar la rama del proceso seleccionado |
| K | Enviar una señal o cambiar la prioridad |
| E | Buscar entropía alta en todos los procesos |
| Alt-N | Mostrar el tráfico del proceso seleccionado en la pantalla de red |
| Space | Pausar o reanudar el muestreo |
| R | Actualizar la vista actual |
| Esc | Volver; en la lista de procesos, cerrar el monitor o regresar a la pantalla de red |

Haz clic en el encabezado de una columna para ordenar, y doble clic en un proceso para abrir su
mapa de memoria. El resumen muestra la memoria residente actual, el valor más alto observado, el
cambio desde que empezó la monitorización, la CPU, el espacio de direcciones virtual y un gráfico
de la memoria residente de los últimos 60 segundos. Los totales de memoria usada, en swap, privada
y compartida aparecen una vez leído el mapa de memoria, si el sistema los proporciona. **Enter** en
el panel de memoria abre el mapa de memoria. El mapa de memoria se lee cuando lo pides; pulsa R
para volver a leerlo después de que el proceso cambie sus asignaciones.

Debajo del gráfico, **Árbol de procesos** lista los procesos de los que desciende el seleccionado,
el propio proceso y todo lo que ha iniciado, cada uno sangrado debajo del que lo inició y con su
memoria residente. Cuando no caben todos, la última línea indica cuántos quedan fuera.

En el árbol que muestra **T**, cada proceso aparece debajo del que lo inició, y **▾** marca un
proceso que tiene otros debajo. **Left** pliega la rama del proceso seleccionado; en un proceso sin
nada debajo, Left pliega la rama a la que pertenece y selecciona el proceso que lo inició. **Right**
despliega una rama plegada y, en una abierta, pasa al primer proceso de esa rama. Un proceso plegado
muestra **▸** y cuántos procesos oculta. Una rama sigue plegada hasta que su proceso termina.
Mientras se aplica un filtro, la lista es plana.

### Señales y prioridad

K abre **Señal o prioridad** para el proceso seleccionado. Pulsa la letra de una opción, o muévete
hasta ella y pulsa Enter. Esc cierra el diálogo sin enviar nada.

| Tecla | Opción | Qué hace |
| --- | --- | --- |
| T | **Terminar** | Envía TERM, que pide al proceso que salga |
| I | **Interrumpir** | Envía INT, lo mismo que Ctrl-C en su terminal |
| H | **Colgar** | Envía HUP; muchos servicios vuelven a leer su configuración |
| K | **Matar** | Envía KILL, que termina el proceso al instante sin dejarle limpiar |
| S | **Detener** | Envía STOP, que pone el proceso en pausa |
| C | **Continuar** | Envía CONT, que reanuda un proceso detenido |
| L | **Bajar prioridad** | Sube su valor nice en 5, hasta 19, para que espere más a la CPU cuando el sistema está ocupado |
| R | **Subir prioridad** | Baja su valor nice en 5, hasta -20 |

Justo antes de enviar, Borg comprueba que el PID sigue perteneciendo al proceso que elegiste,
iniciado a la misma hora, para que un proceso nuevo que haya reutilizado el PID quede intacto. La
línea de estado confirma lo que se envió y la lista vuelve a muestrear al instante. La vista
general indica cuándo un proceso está detenido y muestra su valor **Nice** cuando no es 0.

Puedes enviar señales a tus propios procesos y bajar su prioridad. El sistema rechaza el resto: los
procesos de otro usuario y subir una prioridad, algo que solo puede hacer un administrador.
Entonces Borg ofrece **A Administrador**, que envía la misma opción a través del asistente descrito
en [Acceso de administrador](#administrator-access). Borg no se envía señales a sí mismo; F10 lo
cierra.

### Mapa de memoria

El mapa tiene tres columnas:

- **Memoria usada** es la memoria imputada solo a este proceso: las páginas que ha escrito, más las
  páginas enviadas a swap o comprimidas. Es la cifra que hay que vigilar para detectar fugas, y en
  macOS coincide con la columna *Memoria* del Monitor de Actividad.
- **Residente** son las páginas que están ahora en la RAM, incluidas las compartidas con otros
  procesos.
- **Mapeada** es espacio de direcciones, incluidas las reservas. No dice nada del uso de RAM.

Un proceso puede mapear gigabytes, en su mayoría bibliotecas del sistema compartidas con todos los
demás procesos, y aun así tener una memoria usada de solo unos pocos megabytes. Las regiones
aparecen al principio agrupadas por finalidad: heap y asignaciones, pilas de hilos, el código y los
datos del propio programa, bibliotecas compartidas, archivos mapeados, memoria compartida,
asignaciones del sistema y otras, y rangos reservados y páginas de protección. Los grupos y sus
regiones se ordenan por memoria usada, con las reservas al final. Las bibliotecas y los archivos
mapeados tienen una fila por archivo; despliega un archivo para ver sus regiones. Los nombres de
los grupos se deducen de las etiquetas que da el sistema operativo. Al seleccionar un grupo o un
archivo se explica qué es y se muestran su parte de la memoria usada y sus miembros más grandes.
Enter o Right despliegan un grupo o un archivo; Left lo pliega, o pliega el que contiene la fila
seleccionada. G alterna entre la lista agrupada y la lista sin agrupar.

### Archivos abiertos

**1 Memoria** y **2 Archivos abiertos** alternan entre las pestañas de inspección; también puedes
hacer clic en ellas. La lista de archivos abiertos muestra cada descriptor, su modo de acceso, su
tipo y su ruta o extremo de red; el otro panel muestra todos los detalles, el tamaño del archivo y
el desplazamiento actual cuando están disponibles. En macOS también incluye entradas como `cwd`
(directorio actual) y `txt` (ejecutable mapeado). R actualiza la lista. Cada pestaña conserva su
posición en la lista al cambiar de una a otra. Cuando el acceso está restringido, Borg ofrece
volver a intentarlo como administrador. En un archivo normal, los detalles también muestran una
vista previa de su contenido, como la vista rápida: **Tab** entra en la vista previa para que
puedas desplazarte por ella, y **F3** abre el archivo en el visor. Borg abre el archivo por su ruta
con tus propios permisos y deja en paz el descriptor del proceso.

### Vista previa de la región

Cuando seleccionas una región legible, Borg captura hasta 64 KiB de ella en el otro panel. Si el
principio de la región solo contiene ceros, la captura salta hasta la primera página con datos, y
la línea sobre los bytes indica en qué parte de la región está la ventana. Cuando te mueves rápido,
Borg espera un momento antes de leer; seleccionar otra región borra la vista previa anterior y
cancela su lectura pendiente. Las regiones que no se pueden leer muestran un mensaje.
Pulsa **Tab** o **Enter** para desplazarte por la vista previa. **Esc** ahí vuelve a la lista, y
**Esc** en el mapa vuelve a la lista de procesos con el cursor y el filtro como estaban.

Las series de filas a cero se agrupan en una sola fila, como "· 60K bytes en cero ·". **]** y **[**
saltan al bloque de datos siguiente y anterior, y **>** y **<** capturan los 64 KiB siguientes y
anteriores de la región. **H** alterna entre Hex y texto, **R** vuelve a capturar la
ventana mostrada y **D** resalta lo que ha cambiado desde la captura anterior del mismo proceso y
dirección. La vista de texto muestra el ASCII imprimible y sustituye los demás bytes por puntos.
Las capturas se toman del proceso en vivo sin pausarlo, así que una captura no es una imagen
coherente de todo el proceso. Se quedan en memoria salvo que las exportes. **E** guarda los bytes
en bruto en un archivo nuevo en la carpeta del panel de archivos activo, legible solo por ti.
Los archivos existentes nunca se sobrescriben.

**M** alterna la vista previa entre los bytes y un mapa de páginas de la región: una celda por
página de 4 KiB, o por serie de páginas en una región grande, marcada según lo que contiene:
punteros a la memoria del propio proceso, texto, texto UTF-16, código, datos comprimidos o
cifrados, otros datos, relleno, ceros y páginas que no están en memoria. Una leyenda muestra la
proporción de cada tipo en la región. Cuando el mapa tiene el foco, las flechas, **PgUp**,
**PgDn**, **Home** y **End** mueven un cursor, y **Enter** abre como bytes la página bajo él. Las
regiones de más de 16 384 páginas se muestrean de forma uniforme. En Linux, el mapa pregunta al
sistema qué páginas están en memoria y no lee las demás, así que ni recupera páginas del swap ni
toca páginas nuevas; en macOS, las páginas que nunca se han tocado se leen como ceros. Los tipos se
deducen solo a partir de los bytes, así que tómalos como una pista.

Las páginas cuya entropía llega a 7,2 bits por byte se dibujan en rojo y negrita, o solo en negrita
donde el rojo se leería mal, sea cual sea su tipo. Así destaca el código empaquetado o cifrado en
una región ejecutable, donde Borg cuenta como código todas las páginas que no son ceros ni relleno.
La leyenda muestra su proporción como **entropía alta**, la línea del cursor añade la entropía de la
página bajo él, y la misma línea termina con la entropía más alta de la región. Una nota bajo la
leyenda explica qué suelen significar la entropía alta y la baja. Las cachés comprimidas, las
imágenes, los archivos comprimidos y las claves también tienen entropía alta, así que mira qué
contiene la región antes de sospechar de ella.

### Búsqueda en memoria

Pulsa **/** en el panel de memoria, o **Ctrl-F** en cualquiera de los dos paneles, para buscar en
la memoria. Elige un modo, si deben coincidir las mayúsculas y si quieres buscar en la memoria
legible del proceso o en la captura actual. **Texto** busca en UTF-8, UTF-16 (ambos órdenes de
bytes) y UTF-32 (ambos órdenes de bytes). **Unicode** además normaliza los caracteres de
compatibilidad y las secuencias combinadas: por ejemplo, `𝐁𝐨𝐫𝐠` coincide con `Borg`. No
considera iguales letras distintas solo porque se parezcan. **Hex** acepta pares de
bytes como `42 6F 72 67`.

**Cadenas** e **Indicadores** solo leen la memoria que el propio proceso ha rellenado: memoria que
no está respaldada por un archivo y en la que el proceso puede escribir; en macOS, también se
omiten las regiones en las que nunca ha escrito. **Cadenas** muestra una sola vez cada serie de al
menos seis caracteres ASCII imprimibles o UTF-16LE, con el número de copias en memoria, y omite el
relleno, como caracteres o signos de puntuación repetidos; si escribes una consulta, conserva solo
las cadenas que la contienen. **Indicadores** conserva las cadenas que parecen una URL, una
dirección IP, una dirección de correo electrónico, una ruta de archivo, una línea de comandos, un
secreto (una clave privada, un token de acceso o una asignación de contraseña) o una serie larga de
base64, y etiqueta cada una.

Borg también lee las cadenas de los archivos que el proceso tiene mapeados, hasta 1 GiB, y marca
con • las cadenas que solo están en memoria: texto que el proceso ha construido, recibido o
descodificado, y que ninguno de sus archivos contiene. Primero aparecen los indicadores, luego las
cadenas que solo están en memoria y después las que también contienen sus archivos, y las cadenas
que parecen palabras van antes que el resto. La lista conserva las 2000 mejores. Donde otras
coincidencias muestran su región, cada fila muestra el tipo y el número de copias, y la vista
previa muestra la cadena en su sitio; **C** recorre sus copias. Toma estos resultados como pistas
que merece la pena examinar. Los programas normales también contienen muchas URL, rutas y
claves.

Las coincidencias ocupan el panel de la lista y muestran su dirección, su codificación y su región.
Al seleccionar una coincidencia se previsualizan en el otro panel los bytes que la rodean, con la
coincidencia resaltada. Enter entra en la vista previa, y Esc vuelve a las coincidencias. Esc en
las coincidencias vuelve al mapa de memoria o a la lista de procesos de la que venías. Buscar en
una captura guardada usa sus bytes sin leer el proceso en ejecución. Las búsquedas en un proceso
leen la memoria por partes, informan de los bytes que no pudieron leer y se detienen tras 2000
coincidencias o 60 segundos. **N** continúa desde donde se detuvo la búsqueda, y **Ctrl-C** la
detiene y conserva los resultados. El informe de cobertura separa los bytes buscados de los
ilegibles. Un proceso puede cambiar su memoria mientras se busca en ella.

### Comprobación de integridad del código

**I**, en el mapa de memoria o entre las coincidencias de búsqueda, comprueba si el proceso está
ejecutando el código que contienen sus archivos. Compara el código en memoria con los archivos
desde los que se mapeó e informa de las páginas que difieren, busca en la memoria no respaldada por
archivos cabeceras de imágenes ejecutables (ELF, Mach-O y PE), y enumera la memoria ejecutable no
respaldada por archivos, la memoria que es a la vez escribible y ejecutable, y el código procedente
de archivos eliminados, sin nombre o temporales. También comprueba el proceso en su conjunto:
bibliotecas inyectadas mediante `LD_PRELOAD`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES` o
`/etc/ld.so.preload`, rutas de búsqueda de bibliotecas, un trazador en Linux, un programa cuyo
archivo se ha eliminado o sustituido, y un nombre de proceso distinto del de su programa.

Los hallazgos ocupan el panel de la lista, con las alertas primero, luego las advertencias y
después los avisos. Al seleccionar uno se previsualizan los bytes a los que se refiere, con la
evidencia resaltada, y las filas bajo la lista indican qué otra cosa podría explicarlo. Los
programas que compilan código mientras se ejecutan, como los navegadores y los motores de Java y
JavaScript, siempre contienen código no respaldado por ningún archivo, así que para ellos esos
hallazgos son solo avisos. **R** vuelve a comprobar, **Ctrl-C** detiene una comprobación y conserva
lo encontrado hasta el momento, y **Esc** vuelve atrás. Una comprobación lee como máximo 512 MiB de
código y 1 GiB de otra memoria, durante un máximo de 45 segundos, e indica si se detuvo antes de
tiempo.

En Linux, las tablas de páginas muestran qué páginas de código ha modificado un proceso, así que
solo se leen esas; en macOS, tampoco se leen las regiones sin páginas modificadas. El código de la
caché compartida de macOS no tiene un archivo propio y no se compara. La memoria que una
comprobación no puede leer se contabiliza, y Borg ofrece **A Administrador**. Toma los hallazgos
como pistas, porque los depuradores, los perfiladores, los actualizadores y las herramientas de
seguridad dejan los mismos rastros.

### Analizar la memoria con reglas

**Y**, en el mapa de memoria o entre las coincidencias de búsqueda, analiza la memoria que el propio
proceso ha rellenado con las mismas reglas que un análisis de archivos: las de Borg y las de tu
carpeta de reglas (consulta [Analizar archivos con reglas](#scanning-files-with-rules)). Igual que
**Cadenas**, lee la memoria no respaldada por archivos y la memoria en la que el proceso puede
escribir; en macOS también omite las regiones en las que nunca ha escrito. Una región sin ninguna
página en memoria ni en swap no se lee, y en Linux las tablas de páginas muestran qué páginas no ha
tocado nunca el proceso, así que esas tampoco se leen. Las páginas en swap sí se leen, porque una
carga maliciosa puede esperar ahí. La memoria se lee en bloques de 16 MiB que se solapan 4 KiB, así
que una coincidencia que pasa de un bloque al siguiente también se encuentra. Cada regla aparece una
vez por región, en su primera coincidencia.

Las coincidencias ocupan el panel de la lista, con las alertas primero. Cada una indica la regla y
de dónde viene, **reglas de Borg** o tu archivo, luego los bytes que encontró y lo que busca la
regla, y las filas bajo la lista indican qué otra cosa podría explicarla. Al seleccionar una
coincidencia se previsualizan los bytes que la rodean, con la coincidencia resaltada. Los archivos
de reglas omitidos aparecen al final, con el mensaje del compilador. **R** vuelve a analizar,
**Ctrl-C** detiene el análisis y conserva lo encontrado, y **Esc** vuelve atrás. Un análisis lee
como máximo 1 GiB, durante un máximo de 45 segundos, muestra hasta 2000 coincidencias e indica si se
detuvo antes de tiempo. La memoria que no puede leer se contabiliza, y Borg ofrece **A
Administrador**, que ejecuta el análisis en el programa auxiliar con tu carpeta de reglas.

Un análisis con reglas es una evaluación rápida: un vistazo a un proceso o una carpeta en busca de
patrones que alguien ha descrito. No vigila el sistema, no bloquea nada ni limpia, que es la tarea
de la protección de endpoints. Algunas cosas quedan fuera de su alcance:

- macOS protege la memoria de sus propios programas y de los programas creados con Hardened Runtime
  frente a cualquier otro programa, administradores incluidos (consulta [Acceso de
  administrador](#administrator-access)).
- En Linux, `kernel.yama.ptrace_scope` decide de quién puedes leer la memoria. Con 1, como en
  Ubuntu, solo puedes leer tus propios procesos hijos, así que Borg ofrece **A Administrador** para
  los demás. Con 3, nadie puede leer otro proceso, administradores incluidos.
- Un rootkit de kernel puede ocultar memoria, archivos y procesos a cualquier programa, Borg
  incluido, así que un análisis limpio demuestra poco.
- Un programa empaquetado o cifrado solo muestra su código real mientras se ejecuta. Analizar su
  memoria entonces ve lo que un análisis de su archivo no puede ver.

Borg lee la memoria sin adjuntarse como depurador, con `process_vm_readv` en Linux y `mach_vm_read`
en macOS, así que el proceso no ve ningún trazador, que es lo que buscan la mayoría de las
comprobaciones antidepuración. Los monitores que se ejecutan como administrador, como auditd, las
herramientas eBPF y los clientes de Endpoint Security de macOS, pueden registrar las lecturas de
todos modos. Un análisis de archivos abre cada archivo que lee, y `inotify` y `fanotify` lo
notifican. En Linux, deja intacta la hora de acceso de los archivos que son tuyos.

### Análisis de entropía de todos los procesos

**E** en la lista de procesos abre el **Análisis de entropía**, que lee una muestra de las páginas
de todos los procesos en busca de bytes tan densos como el código empaquetado. Pulsa la letra de una
opción, o muévete hasta ella y pulsa Enter. Esc cierra el diálogo.

| Tecla | Opción | Qué lee |
| --- | --- | --- |
| C | **Código de todos los procesos** | Las regiones ejecutables, donde aparece el código empaquetado o cifrado |
| M | **Toda la memoria de todos los procesos** | Todas las regiones legibles, así que también datos como claves y archivos comprimidos |
| L | **Últimos resultados** | Nada nuevo: vuelve a mostrar los hallazgos del último análisis |

Borg lee hasta 1024 páginas de 4 KiB de una región y 32 768 de un proceso, repartidas de forma
uniforme. Un proceso con muchas regiones recibe menos páginas por región, pero nunca menos de 16.
Una región con al menos una página de 7,2 bits por byte o más es un hallazgo. Los hallazgos ocupan
el panel de la lista: primero el código, luego la memoria que escribió el proceso y por último los
archivos mapeados tal como están en el disco, y dentro de cada grupo primero la región con mayor
proporción de páginas con entropía alta. El código con el 70 % o más de sus páginas con entropía
alta aparece en negrita, según la misma regla que la búsqueda de [ejecutables
empaquetados](#finding-packed-executables). Cada fila indica el tipo (**código**, **escrita** o
**archivo**), esa proporción con la entropía más alta, y el nombre del proceso, el PID y la región.
El otro panel muestra las direcciones, el tamaño y el acceso de la región, cuántas páginas de la
muestra tienen entropía alta, dónde empieza la primera de ellas y qué suelen ser esos bytes. Se
conservan los 2000 hallazgos más fuertes, y el resumen cuenta los más débiles que se omitieron.

**Enter** en un hallazgo abre el mapa de memoria del proceso en esa región, con el mapa de páginas
activado y el cursor en la primera página densa de la muestra, para que veas los bytes que la
rodean. **Esc** vuelve al mapa y, otra vez, a los hallazgos.

Borg lee 4 procesos a la vez. Una región sin ninguna página en memoria ni en swap no se lee, y en
Linux las tablas de páginas muestran qué páginas están en memoria, así que el análisis no toca
páginas nuevas ni recupera las que están en swap. Se omiten las cachés que todos los procesos
mapean igual: la caché compartida de dyld en macOS y las páginas que el kernel mapea en todos los
procesos en Linux. Un archivo mapeado sin cambios, como una biblioteca compartida, solo se lee en
el primer proceso que lo mapea.

Un análisis se detiene a los 90 segundos y deja el resto para **N**, que sigue con los procesos a
los que no llegó. **Ctrl-C** lo detiene antes y conserva los hallazgos, y **R** vuelve a analizar
desde el principio. Encima de los hallazgos, una línea cuenta los procesos analizados y los
hallazgos, y la siguiente los procesos omitidos: ilegibles, protegidos y no alcanzados. En macOS,
vmmap tarda unos segundos en listar las regiones de un proceso, así que analizar todos los procesos
puede requerir varias rondas de **N**.

Los procesos cuya memoria protege macOS se cuentan y se omiten, porque nadie puede leerlos. Para los
procesos que Borg no pudo leer, ofrece **A Administrador**, que los analiza en el programa auxiliar
durante un máximo de 60 segundos, para que el auxiliar termine dentro de su límite de 2 minutos
incluso tras una petición de contraseña lenta. Los bytes densos son habituales: las cachés
comprimidas, las imágenes, las fuentes y las claves tienen entropía alta por naturaleza, así que toma
los hallazgos como pistas y lee los detalles antes de sospechar de un proceso.

### Acceso de administrador

Cuando la lectura de un mapa de memoria o de una captura está restringida, una búsqueda no pudo
leer parte de la memoria, un análisis de entropía no pudo leer algunos procesos, o el sistema
rechaza una señal o un cambio de prioridad, Borg ofrece
**A Administrador**. En macOS, esto abre la petición de
autorización del sistema; en Linux usa `pkexec` y necesita que haya un agente de autorización en
ejecución. Solo un asistente de corta duración se ejecuta con privilegios elevados. Borg sigue
ejecutándose con normalidad, y el asistente solo devuelve datos del mapa, una cantidad limitada de
bytes o los hallazgos de un análisis, o envía la señal o la prioridad que elegiste.

macOS protege la memoria de sus propios programas (Protección de la Integridad del Sistema) y de los
programas compilados con Hardened Runtime frente a cualquier otro programa, administradores
incluidos. Borg lo detecta a partir de la firma del programa: los detalles del proceso indican
**Memoria protegida**, una lectura rechazada explica el motivo, y no se ofrece **A Administrador**,
ya que no serviría de nada. Sus mapas de memoria y sus archivos abiertos se siguen mostrando.
Tampoco se ofrece cuando Borg ya se ejecuta como administrador. En Linux, la política de seguridad
puede seguir denegando el acceso; Borg muestra el error y el monitor de procesos sigue funcionando
con normalidad.

Un proceso se identifica junto con su hora de inicio, de modo que un proceso nuevo que reutiliza un
PID no hereda el historial de otro. Los mapas de memoria, los resultados de inspección, las
capturas y el historial tienen límites de tamaño. El nombre Borg Commander permanece visible en la
esquina inferior derecha en todas partes, incluidos la consola, el visor, el monitor, los diálogos y
el salvapantallas.

### Alertas

Borg puede vigilar lo que arranca en una máquina y avisarte cuando algo parece mal. Activa
**Vigilar: arranques de procesos inusuales** en F9 → Opciones → Complementos y arranca el servicio
en segundo plano con `borg service start`. El vigilante vive en ese servicio, así que sigue
vigilando sin ninguna terminal abierta, y todos los Borg de la máquina leen las mismas alertas.

**Alt-L** o **Comando → Alertas** abre la página. La lista ocupa el panel activo, con la alerta
más reciente primero, y el otro panel muestra la alerta seleccionada por completo. Cada fila
muestra la hora, la regla que se disparó, el programa, el proceso padre que lo inició, el usuario
con el que corrió y qué ha sido del proceso. La línea sobre la lista cuenta las alertas y cuántas
no ha leído nadie todavía.

Los detalles dicen en una frase por qué se disparó la regla y después el comando, el programa, el
proceso padre, el usuario, a dónde llevaban los flujos estándar y dónde está el proceso ahora. En
**Árbol de procesos** cada padre aparece encima de su hijo, de modo que puedes leer el camino
desde un servicio hasta el comando que te preocupa. **Conexiones** enumera los sockets que el
proceso tenía en ese momento, **Hash del programa** es el SHA-256 de su programa, y **Reglas**
muestra lo que las reglas de Borg y las tuyas encontraron en ese programa (consulta
[Analizar archivos con reglas](#scanning-files-with-rules)).

Un comando de más de 2 KiB conserva su principio y su final, 2 KiB en total, con `…` entre ambos.
Los comandos de sus procesos padre y los valores de sus variables de precarga se cortan del mismo
modo. Así las 200 alertas más recientes caben en el registro por largos que sean sus comandos, y un
primer argumento largo no puede sacar de la vista el final de un comando. Ctrl-C copia el comando
tal como lo guarda el registro.

| Tecla | Acción |
| --- | --- |
| Enter | Mostrar el proceso en el monitor de actividad |
| C | Dejar que un proceso congelado siga corriendo |
| K | Terminar el proceso, después de una pregunta |
| A | Permitir arranques como este desde ahora |
| `/` | Filtrar las filas; ↑↓ siguen moviendo, Enter mantiene el filtro, Esc lo borra |
| R | Volver a leer el registro |
| Ctrl-C | Copiar el comando que ejecutó el proceso |
| Alt-W | Maximizar la lista |
| Esc | Borrar el filtro y después cerrar la página |

El filtro mantiene las alertas que contienen todas las palabras que escribas, en la regla, su
nombre corto, el motivo, el programa, el proceso padre, el usuario, el comando y lo que hizo Borg.
Pon `!` delante de una palabra para dejar fuera lo que encuentra: `shell-on-socket !root` mantiene
las shells remotas de todos los demás usuarios.

Las alertas viven en `alerts.json` en la carpeta de estado, accesible solo a su propietario, las
200 más recientes. Borg mantiene el archivo dentro de 8 MiB, lo máximo que vuelve a leer, y deja
fuera las alertas más antiguas cuando no caben. La ventana **Archivos de Borg** las cuenta en
**Estado** (consulta [Archivos de Borg](#borgs-files)). Abrir la página marca como leídas las
alertas que muestra, y la cuenta de alertas sin leer sigue al registro mientras Borg esté abierto,
con la página abierta o sin ella.

Cuando Borg encuentra un `alerts.json` que no puede leer, como uno que una versión anterior dejó
crecer por encima de 8 MiB, le cambia el nombre a `alerts-unreadable.json` antes de escribir un
registro nuevo, así que las alertas que contiene siguen en el disco. Uno posterior recibe un nombre
numerado como `alerts-unreadable (2).json`. La página de alertas indica el archivo debajo de la
lista mientras siga ahí, así que bórralo cuando lo hayas revisado.

### Qué busca el vigilante

Cada regla nombra una cosa rara de un proceso que acaba de arrancar. El nivel dice cuánto
significa: **alerta** rara vez es inocente, **aviso** suele tener un motivo corriente, **nota**
está ahí para que el retrato quede completo.

| Regla | Qué la dispara | Nivel |
| --- | --- | --- |
| **Shell en un socket de red** | Una shell cuya entrada, salida o errores van a un socket de red, que es el aspecto de una shell remota. Un socket que se queda en la máquina, como el que `systemd` da a la salida de un servicio para el journal, no cuenta | alerta |
| **Shell remota en un terminal** | Una shell en un terminal que un programa abrió para ella, donde la entrada, la salida o los errores de ese programa van a un socket de red, o el programa es un repetidor como `socat` o `nc`. Es una shell remota convertida en un terminal completo, como la que crea `pty.spawn` de Python | alerta |
| **Programa borrado mientras corre** | El archivo del programa ya no existe, así que nada en el disco dice qué es el proceso | alerta |
| **Programa sin archivo** | Borg no encuentra ningún archivo de programa para el proceso | alerta |
| **Biblioteca forzada en un programa** | `LD_PRELOAD`, `LD_AUDIT` o `DYLD_INSERT_LIBRARIES` está fijado en su entorno | alerta |
| **Shell desde un servicio** | Un servidor web, un entorno de ejecución de aplicaciones o una base de datos inició una shell | aviso |
| **Shell sin terminal** | Una shell sin terminal en ninguno de los dos lados, así que nadie la escribió | aviso |
| **Shell en el terminal de un programa** | Una shell en un terminal que un programa abrió para sí mismo y en el que nadie inició sesión, como el que abren `expect` o `docker exec` | aviso |
| **Exploración de la máquina** | Cuatro comandos distintos como `whoami`, `id`, `uname` y `ss` del mismo programa en menos de una hora, sin nadie con la sesión iniciada en el terminal donde se ejecutan. Los comandos que ejecuta a través de una shell tras otra cuentan juntos, así que también se detecta una web shell que inicia `sh -c` para cada comando | aviso |
| **Programa en una carpeta temporal** | El programa está en `/tmp`, `/var/tmp`, `/dev/shm` o `/run/shm` | aviso |
| **Nombre distinto del programa** | El proceso se llama de una manera y ejecuta otra cosa | aviso |
| **El padre nunca lo había iniciado** | Este padre nunca ha iniciado este programa en esta máquina | nota |

De cada proceso Borg lee el programa que ejecuta y si su archivo se borró, el comando, el usuario,
el terminal al que pertenece y quién lo abrió, a dónde llevan su entrada, salida y errores estándar,
las variables de precarga de su entorno y la cadena de padres que tiene encima. Un socket cuenta como
socket de red cuando la red del proceso lo lista como TCP o UDP.

Si alguien inició sesión en un terminal se decide igual que para congelar (consulta
[Congelar un proceso](#freezing-a-process)): por SSH, en la consola, en `tmux` o `screen`, mediante
`sudo` o `su`, o en una ventana de terminal. Un terminal que un programa abrió para sí mismo no
cuenta, ya lo abriera el servidor remoto de un editor, `docker exec` o una shell inversa.

Borg aprende la máquina antes de actuar. Durante los primeros 7 días registra qué padre inicia qué
programa aquí y no congela nada, y la columna Respuesta dice **Habría congelado** donde lo habría
hecho. Después, un par que ya ha visto se queda en silencio para las reglas que se pueden aprender:
las tres reglas de las shells, el nombre distinto y el padre que nunca lo había iniciado. La serie de
comandos se aprende por el programa del que viene: se queda en silencio cuando ese programa ejecutó
cada uno de sus comandos durante la semana de aprendizaje, así que un agente de monitorización que
ejecuta `df` y `ps` todo el día se queda en silencio, y un servidor web que nunca ejecutó `id` no. Las
cinco reglas de nivel alerta se disparan siempre, porque una shell en un socket o una
biblioteca forzada nunca se vuelven normales por ocurrir a menudo. Todo lo que se ejecuta durante
esa semana cuenta como normal, así que activa el vigilante mientras confías en la máquina. Los
arranques en los que se disparó una regla que el aprendizaje no puede silenciar quedan fuera de lo
que Borg aprende.

**A** en una alerta permite arranques como ese desde entonces, por su programa y su padre. Esos
permisos van a `watch-allowed.json` en la carpeta de estado, que el vigilante lee y adopta en menos
de un minuto. La sección `[watch]` de la configuración guarda lo mismo a mano (consulta
[Configuración](#configuration)). Borg anota como máximo 20 procesos por minuto y congela como
máximo 5, y la línea sobre la lista dice cuántos hallazgos omitió cuando se dispararon más a la vez.
Un proceso que Borg congela siempre queda anotado, por muchas alertas que hayan llegado antes, para
que puedas dejarlo continuar. Las notas se cuentan aparte, así que una serie de ellas nunca ocupa el
lugar de un aviso, y también quedan anotados hasta 20 procesos por minuto que Borg habría congelado
por encima del límite de 5.

### Congelar un proceso

Una vez terminado el periodo de aprendizaje, Borg congela el proceso en el que se disparó una
regla de nivel alerta o aviso. Congelar envía una señal de parada al proceso y a todo lo que está
debajo de él. Un proceso congelado no ha terminado, así que su servicio sigue corriendo,
`systemd` y Docker no tienen nada que reiniciar, y lo que estaba a punto de hacer espera donde
está hasta que lo mires. La columna Respuesta dice **Congelado**, **C** lo deja continuar y **K**
lo termina. Terminarlo es definitivo, así que Borg pregunta primero.

La congelación empieza cuando hayas visto todas las alertas de la semana de aprendizaje que dicen
**Habría congelado**, o 3 días después de que termine la semana, lo que ocurra primero. Abrir la
página de alertas las marca como leídas, y el vigilante lo nota en menos de un minuto. Así puedes
revisar lo que encontró la semana y permitir un programa inofensivo con **A** antes de que nada se
detenga, y una máquina que nadie mira sigue protegida. Mientras tanto, el pie de la página dice
hasta cuándo espera la congelación, y los detalles de una alerta nueva dicen que está esperando.

Borg nunca congela el primer proceso del sistema, ni a sí mismo ni nada que haya iniciado, ni un
programa de la lista que nunca congela (`init`, `systemd`, `sshd`, `sudo`, `su`, `login` y
parecidos, más cualquier nombre que añadas en `keep-running`), ni un proceso en un terminal en el
que alguien inició sesión, ni un arranque que hayas permitido. Una nota nunca detiene un programa.
Los detalles dicen cuál de estos motivos mantuvo el proceso en marcha.

Un programa de la lista incorporada solo se respeta donde el sistema guarda sus programas, como
`/usr` y `/bin`, así que una copia llamada `sshd` en otra carpeta se congela como cualquier otra
cosa. Los nombres que añadas en `keep-running` coinciden esté donde esté el programa. Un terminal
cuenta cuando alguien inició sesión en él: por SSH, en la consola, en `tmux` o `screen`, a través de
`sudo` o `su`, o en una ventana de terminal. Un multiplexor de terminal solo cuenta para root y para
las cuentas de personas, numeradas desde `UID_MIN` en `/etc/login.defs` (1000 en la mayoría de los
sistemas), porque una cuenta de servicio también puede iniciar uno. Un terminal que un programa
abrió para sí mismo no cuenta, lo que impide que una shell remota se esconda detrás de uno. Los
terminales que abre el servidor remoto de un editor o `docker exec` tampoco cuentan, así que si Borg
congela algo que ejecutaste ahí, **C** lo deja continuar y **A** lo permite.

Borg comprueba la identidad de un proceso, su número junto con el momento en que arrancó, justo
antes de enviarle una señal, así que un número que otro proceso haya tomado entretanto se queda
tranquilo.

### El vigilante como root

Sin más derechos, Borg sabe solo de los procesos de su propio usuario, mirando la lista de
procesos cuatro veces por segundo, y la página lo dice debajo de la lista. Un comando breve puede
desaparecer antes de la mirada siguiente.

Para saber de cada proceso en el momento en que arranca, instala la parte que corre como root:

```sh
borg service watch install
```

Esto pide tu contraseña una vez e instala una unidad aparte, `borg-commander-watch.service`, que
ejecuta `borg --watch-root`. Esa parte carga en el kernel unos pequeños programas propios de Borg,
que leen cada arranque de programa antes de que el programa nuevo se ejecute: el programa, la línea
de comandos tal como se pasó, el proceso que lo inició y el usuario. Un comando que termina al
instante se informa igualmente, y también la línea de comandos de un proceso que la reescribe
después. La parte root lee `/proc` para el resto, y detiene, continúa o termina un proceso cuando
Borg se lo pide. No hace nada más: la unidad renuncia a todas las capacidades que no necesita, y la
interfaz de Borg sigue corriendo como tú. Los dos lados hablan por un socket de la carpeta de estado
al que solo llega su propietario, Borg acepta una conexión únicamente de un proceso que corre como
root, y la parte root se conecta únicamente a un socket que pertenece al usuario para el que se
instaló. `borg service watch uninstall` la quita, y `borg service watch status` imprime de dónde
vienen los eventos y qué se ha encontrado.

Los programas necesitan Linux 5.8 o posterior. En un kernel más antiguo, o donde la parte root no
puede cargarlos, escucha los eventos de procesos del kernel y lee todo de `/proc`, y la página lo
indica bajo la lista. Después de actualizar Borg, vuelve a ejecutar `borg service watch install`
para que la unidad reciba los permisos que necesite la nueva versión.

Las alertas de cualquiera de los dos lados son las mismas alertas, en el mismo registro. Un
proceso de otro usuario solo lo puede congelar la parte root, así que reanudarlo o terminarlo
desde la página pasa por el vigilante del servicio. Cuando nada puede alcanzarlo, la página dice
que solo el propietario o un administrador puede hacerlo.

El vigilante es por ahora una función de Linux. En macOS la página se abre y lee su registro, y el
origen dice que nada está vigilando.

### Alertas fuera de la máquina

La página Alertas vive en la máquina que vigila, así que quien se haga con esa máquina puede borrar
el registro. Envía cada alerta a otro sitio en cuanto ocurre: una copia que ya llegó queda fuera de
su alcance, y el latido te avisa cuando el vigilante se queda callado. Añade `[watch.notify]` a la
configuración del servidor, con cualquiera de estos destinos o con varios. Todas las direcciones
usan `https://`, o `http://` hacia el propio servidor:

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

Luego ejecuta `borg service watch notify`. Envía un mensaje de prueba a cada destino y muestra lo
que respondió cada uno.

- **Telegram**: crea un bot con @BotFather, que te da el token. Envía un mensaje al bot, abre
  `https://api.telegram.org/bot<token>/getUpdates` y toma `chat.id` de la respuesta.
- **ntfy**: suscríbete en la app de ntfy a un tema con un nombre largo que nadie pueda adivinar, o
  a uno de tu propio servidor ntfy.
- **Webhook**: Borg envía cada mensaje a la dirección como JSON. Con `secret`, cada petición lleva
  `X-Borg-Signature: t=<time>,v1=<signature>`: el HMAC-SHA256, con el secreto, de la hora, un punto
  y el cuerpo, para que tu receptor distinga los mensajes de Borg de los falsificados.
  `X-Borg-Event` dice qué es el mensaje: `alert`, `evidence` (el SHA-256 del programa y las
  coincidencias de reglas, que siguen a una alerta cuando Borg ha leído el programa), `started`,
  `armed` o `test`.
- **Latido**: Borg llama a la dirección cada `minutes` minutos mientras el vigilante funciona y
  añade `/fail` cuando algo va mal: el hilo que calcula el hash de los programas se detuvo, un
  destino lleva 30 minutos sin aceptar mensajes, Borg abandonó uno, o Borg no pudo leer su bandeja
  de salida. Un servicio como healthchecks.io te avisa cuando las llamadas se detienen, que es lo
  que ocurre cuando alguien mata al vigilante.

El servicio envía cada alerta pocos segundos después de anotarla, y también avisa cuando el
vigilante arranca y cuando empieza la congelación. Las alertas de nivel nota se quedan en la
página salvo que `level` sea `notice`. Un mensaje que un destino rechaza espera en
`watch-outbox.json`, en la carpeta de estado, y se reintenta, a los 5 segundos la primera vez y
cada 30 minutos como máximo. Borg guarda hasta 500 mensajes en espera, menos cuando los largos
harían que el archivo pasara de 16 MiB, e intenta cada uno durante 48 horas. El mensaje siguiente
dice cuántos abandonó.

Un mensaje lleva como máximo 1000 bytes de la línea de comandos, de la ruta del programa y del
motivo de la alerta. Uno más largo conserva el principio y el final, con puntos suspensivos entre
ambos, así que rellenar el principio de un comando no oculta lo que hace.

Cuando Borg encuentra un `watch-outbox.json` que no puede leer, cambia el nombre del archivo a
`watch-outbox-unreadable.json` y empieza una bandeja de salida nueva. Uno posterior recibe un nombre
numerado, como `watch-outbox-unreadable (2).json`. Los mensajes que esperaban se quedan en ese
archivo y nunca se envían, así que el latido falla una vez y dice por qué, y la numeración de cada
destino vuelve a empezar en 1. Revisa el archivo y luego bórralo.

Cada destino numera sus mensajes desde 1, y el JSON de cada mensaje lleva en `previous` el SHA-256
del anterior. Un salto en la numeración, o un `previous` que no coincide, significa que un mensaje
se perdió o se cambió por el camino. La numeración se hace en el servidor, así que no detecta a
quien controla el servidor.

Los tokens están en la configuración, que cualquier cosa que se ejecute como tu usuario en el
servidor puede leer. Con ellos, alguien puede enviar mensajes que parecen de Borg y seguir la
numeración sin saltos, y un bot de Telegram puede borrar lo que envió en las últimas 48 horas. Quien
se haga con el servidor también puede borrar los mensajes que aún esperan para salir. Lo que llegó
antes queda como estaba, y un webhook a una máquina que controlas lo conserva tal como llegó. Cuando
el vigilante se queda callado, es el latido el que te avisa.

### Lo que el vigilante no ve

El vigilante juzga cada programa cuando arranca, y eso deja algunas cosas fuera de su vista.
Conócelas antes de confiar en él.

- El código que se ejecuta dentro de un programa que ya está en marcha no inicia nada nuevo, así que
  ninguna regla lo mira. Una aplicación web cuyo propio intérprete ha sido tomado conserva el nombre
  y el padre de siempre.
- Los procesos que ya corrían antes de que arrancara el vigilante nunca se examinan, y todo lo que
  se ejecuta durante la semana de aprendizaje cuenta como normal.
- Sin el vigilante como root, Borg solo ve tus propios procesos, y un comando que termina en menos
  de un cuarto de segundo puede desaparecer antes de que Borg mire. Los servicios que se ejecutan
  como otro usuario, como `www-data` o `postgres`, necesitan el vigilante como root.
- Antes de Linux 5.8, o donde la parte root no puede cargar sus programas, Borg lee cada proceso
  nuevo en `/proc` después de que el kernel lo informa. Un comando que termina al instante puede
  haber desaparecido para entonces, y un proceso puede cambiar antes su línea de comandos.
- Algunos kernels no envían eventos de procesos, como en ciertas máquinas virtuales y entornos de
  contenedores. Borg ejecutándose como root y el vigilante como root miran entonces todos los
  procesos cuatro veces por segundo, y un comando breve puede terminar antes de que miren.
- Una ráfaga de procesos nuevos puede llenar la cola entre el kernel y Borg, y entonces el kernel
  descarta eventos, así que algunos arranques nunca se examinan.
- Un comando que terminó antes de que Borg pudiera leer `/proc` lleva solo lo que leyeron los
  programas: su programa, su línea de comandos, su proceso padre y su usuario. Su terminal, sus
  flujos estándar y sus variables de precarga terminaron con él, así que las reglas sobre ellos no
  pueden activarse. **Programa borrado mientras corre** solo se activa con él cuando su archivo ya
  no existía al arrancar, como en un programa ejecutado desde la memoria.
- Los programas guardan los primeros 4 KiB de una línea de comandos, y Borg toma una más larga de
  `/proc` mientras el proceso sigue ahí. Encuentran la línea de comandos y el archivo del programa
  gracias a la información de tipos que el kernel publica en `/sys/kernel/btf/vmlinux`. En un kernel
  compilado sin ella, la línea de comandos también sale de `/proc`. Un comando que terminó antes se
  nombra entonces por la ruta con la que se inició, que puede ser relativa a su carpeta de trabajo o
  pasar por un enlace simbólico, así que **Programa en una carpeta temporal** puede pasarlo por
  alto.
- Cualquier cosa que se ejecute como el usuario del vigilante puede detener el vigilante, cambiar
  sus archivos en la carpeta de estado o dejar continuar un proceso congelado con `kill -CONT`.
  Congelar te da tiempo para mirar, y el dueño del proceso todavía puede deshacerlo.
- Un proceso en un terminal en el que alguien inició sesión nunca se congela, aunque la sesión usara
  una contraseña o clave robada. Las reglas siguen disparándose y anotando alertas.
- **Exploración de la máquina** cuenta los comandos de un programa en una hora. Quien espera más
  entre comandos, o ejecuta cada uno desde un programa distinto, se queda por debajo.
- Un proceso que se separa de su padre antes de que Borg congele al padre sigue en marcha.
- Sin `[watch.notify]`, las alertas solo te llegan cuando abres la página. Con él, quien se haga
  con la máquina en los segundos que siguen a una alerta todavía puede frenar el mensaje antes de
  que salga, y entonces es el silencio del latido lo que te avisa.

## Red

**Alt-N** o **Comando → Red** muestra lo que hace este equipo en la red: los puertos abiertos y
quién puede llegar a ellos, los procesos que se comunican y a qué velocidad, cada conexión y las
interfaces de red. Lo proporciona el complemento Sistema incluido, igual que el monitor de
actividad. La lista ocupa el panel activo, y el otro panel muestra en detalle la fila seleccionada,
con gráficos de los últimos 60 segundos. Borg toma una muestra por segundo mientras la pantalla está
abierta y se detiene al cerrarla. Las velocidades aparecen a partir de la segunda muestra, y cada
una es la media de los últimos 3 segundos, así que una lista ordenada por tráfico se queda quieta el
tiempo suficiente para leerla.

Pulsa de **1** a **4**, o haz clic en una pestaña, para cambiar de página:

- **1 Puertos** lista cada puerto en el que escucha un proceso, ordenados por número. **Alcance**
  indica quién puede conectarse: **Todas las redes** para un puerto abierto en todas las
  direcciones, la dirección para un puerto vinculado a direcciones concretas, y **Este equipo** para
  un puerto al que solo llegan los programas de este equipo. Los puertos abiertos a todas las redes
  se destacan, porque otras máquinas pueden encontrarlos. **Con.** cuenta las conexiones que
  entraron por el puerto, y los detalles las listan con su tráfico.
- **2 Procesos** lista los procesos que usan la red, los más activos primero, con sus velocidades de
  descarga y subida y su número de conexiones. Los detalles añaden lo que el proceso ha transferido
  desde que se abrió la pantalla y cada conexión que mantiene.
- **3 Conexiones** lista cada conexión con su proceso, el otro extremo y sus velocidades. Un panel
  ancho añade el protocolo y el estado TCP. Los detalles muestran ambos extremos, el nombre del
  host, el servicio y los bytes recibidos y enviados.
- **4 Interfaces** lista las interfaces de red con su tipo (**Wi-Fi**, **Ethernet**, **Loopback**,
  **Túnel**, **Puente** o **Virtual**), su primera dirección y sus velocidades. ● marca una interfaz
  activa. Los detalles añaden todas sus direcciones, la MTU, los bytes y paquetes que la interfaz ha
  recibido y enviado hasta ahora, y sus errores.

El pie muestra la velocidad total de descarga y subida del equipo en todas las interfaces salvo la
de loopback. Borg busca en segundo plano los nombres de las direcciones remotas y muestra el nombre
en lugar de la dirección en cuanto lo conoce.

| Tecla | Acción |
| --- | --- |
| 1 / 2 / 3 / 4 | Mostrar Puertos / Procesos / Conexiones / Interfaces |
| `/` | Filtrar las filas; ↑↓ siguen moviendo, Enter conserva el filtro, Esc lo borra |
| T / N / P / C | Ordenar por tráfico / nombre / puerto / conexiones; pulsa de nuevo para invertir |
| Enter | Mostrar el proceso de la fila seleccionada en el monitor de actividad |
| K | Mostrar el proceso con **Señal o prioridad** abierto |
| R | Capturar el tráfico de la fila seleccionada o detener la captura en curso |
| Ctrl-C | Copiar la dirección del puerto, el PID, el otro extremo o la dirección de la interfaz |
| Tab | Alternar entre la lista y los detalles |
| Space | Pausar o reanudar el muestreo |
| Alt-W | Maximizar el panel con el foco |
| Esc | Volver a la lista, después borrar el filtro y después cerrar la pantalla o regresar al monitor de actividad |

P ordena las páginas que tienen puerto, y C las que cuentan conexiones. Haz clic en el encabezado de
una columna para ordenar por ella, y doble clic en una fila para mostrar su proceso. Enter en una
fila sin proceso pasa a los detalles. El monitor de actividad que abren Enter y K tiene el proceso
seleccionado, así que su memoria, sus señales y su prioridad quedan a una tecla (consulta [Señales y
prioridad](#signals-and-priority)). Esc allí te devuelve a la pantalla de red, a la fila donde
estabas y con sus gráficos.

También funciona al revés. Alt-N en el monitor de actividad abre esta pantalla en la fila del
proceso seleccionado de **2 Procesos**, con su tráfico y sus conexiones en los detalles, y Esc
vuelve al monitor. Si el proceso no tiene conexiones ni puertos abiertos, un aviso te lo dice.

### Filtrar

El filtro conserva las filas que coinciden con todas las palabras que escribes. Una palabra suelta
coincide con el proceso, el PID, el puerto, el servicio, la dirección, el nombre del host, el
estado, el protocolo o una etiqueta que muestra la fila, así que `postgres`, `5432`, `https` y
`example.com` funcionan. Una palabra con nombre de campo busca solo en ese campo:

| Palabra | Conserva |
| --- | --- |
| `port:443`, `port:ssh` | Filas con ese número de puerto o servicio |
| `proc:node`, `process:node` | Filas cuyo nombre de proceso contiene el texto |
| `pid:4100` | Filas de ese PID |
| `host:example` | Filas cuya dirección o nombre de host contiene el texto |
| `state:listen`, `state:time` | Filas TCP en un estado que contiene el texto |
| `proto:udp`, `proto:tcp` | Filas de ese protocolo |
| `if:en0`, `interface:en0` | Interfaces cuyo nombre contiene el texto |

Pon `!` delante de una palabra para descartar las filas que coinciden con ella: `!state:time_wait`
oculta las conexiones que se están cerrando, y `!proc:browser` oculta un programa. El filtro se
aplica a todas las páginas y, mientras escribes, indica cuántas filas de la página conserva.

### Capturar tráfico

Pulsa **R** para grabar los paquetes de la fila seleccionada con `tcpdump`, que macOS trae de serie
y Linux instala con el paquete `tcpdump`. El diálogo **Capturar tráfico** parte de esa fila:

- **Tráfico** es lo que guarda tcpdump. En **1 Puertos** es el puerto y las conexiones que entraron
  por él, y en **3 Conexiones** esa conexión en ambos sentidos. En **2 Procesos** es **Todo el
  tráfico, mostrado para** ese proceso: tcpdump no puede elegir paquetes por proceso, así que Borg
  graba toda la interfaz y abre el resultado en el proceso. **Todo el tráfico de la interfaz**
  está siempre como segunda opción.
- **Interfaz** empieza en la que tiene la dirección de la fila o, si no la hay, en la más activa.
- **Duración** es de 5, 10, 30 o 60 segundos, o **Hasta detenerla**.
- **Guardar** empieza en **Inicio de cada conexión, hasta 1 MiB**: los paquetes de cada conversación
  se guardan completos hasta que llevan 1 MiB de datos, y a partir de ahí Borg guarda solo sus
  cabeceras. Las peticiones cortas quedan completas, y una descarga larga o una videollamada no
  llenan el archivo. **Paquetes completos** guarda todo, y **Cabeceras, los primeros 128 bytes de
  cada paquete** guarda el principio de cada paquete para los archivos más pequeños. Borg recorta
  los paquetes a medida que escribe el archivo, que sigue siendo un pcap o pcapng normal, y cada
  paquete recortado conserva su longitud en la red.
- **Claves TLS** es **No**, **Definir SSLKEYLOGFILE en el shell de Borg** para guardar las claves
  TLS de los programas que inicies mientras la captura se ejecuta, o **Pasar el HTTPS por el proxy
  de Borg con una CA temporal** para programas que no escriben sus claves (consulta
  [Descifrar TLS](#decrypting-tls)).
- **Ejecutar como** aparece donde Borg puede pedir al sistema la contraseña de un administrador.
  **Tu usuario** ejecuta tcpdump como tú, y **Administrador** lo ejecuta como root después de que
  escribas tu contraseña en el diálogo del sistema. En macOS la opción se llama **Administrador,
  con el proceso de cada paquete**: tcpdump escucha entonces a través de `pktap` de macOS, que marca
  cada paquete con el proceso que lo envió o recibió.

↑↓ eligen un campo, ←→ o Space lo cambian, Enter inicia la captura y Esc la cancela. Con el ratón,
haz clic en un campo para elegirlo y otra vez para cambiarlo. Mientras la captura se ejecuta, el pie
cuenta sus paquetes, sus bytes y los segundos que quedan, y **R** la detiene antes de tiempo. Una
captura también se detiene a los 200.000 paquetes o 256 MiB, y cuando se llena el disco, con lo que
ya guardó. Para dejar espacio al resto del sistema, define `keep-free-mib` en la sección `[network]`
del archivo de configuración: entonces la captura no empieza cuando al disco de la carpeta le quedan
menos de esos MiB libres, y se detiene cuando el espacio libre baja de ahí. Pasados sus primeros 3
segundos, el pie dice cuánto falta para que el primero de estos límites detenga la captura al ritmo
actual, como **unos 30 s hasta 256 MiB** o **unos 2 min hasta llenar el disco**, cuando eso llega
antes del final de la propia captura y dentro de la hora. Cuando un límite detuvo la captura, un
aviso dice cuál. Si cierras la pantalla antes, la captura se detiene y un aviso dice dónde la guardó
Borg.

Borg guarda cada captura como `capture-<date>-<time>.pcap` en la carpeta del panel que cubre la
pantalla de red, legible solo por ti, para que Wireshark y otras herramientas puedan abrirla
después. Una captura que marca cada paquete con su proceso se guarda como
`capture-<date>-<time>.pcapng`, el formato que puede llevar esas marcas. Lo mismo pasa con una
captura que guardó claves TLS, una vez que Borg las pone dentro. Una captura contiene todo lo que llevaban los paquetes, así que guárdala con el mismo
cuidado que esos datos. Cada captura queda también en la lista de [Archivos de Borg](#borgs-files).

Un agente conectado mediante `borg mcp` también puede pedir una captura, que también espera tu
aprobación (consulta [Qué puede hacer el agente](#what-the-agent-can-do)).

Cuando la captura termina, el visor de tráfico ocupa el lugar de las páginas en vivo, con una página
para cada forma de mirarla:

- **1 Procesos** muestra cada proceso con los hosts con los que habló, sus paquetes y sus bytes.
  Cuando los paquetes llevan su proceso, cada paquete pertenece al proceso que lo envió o recibió.
  Así, hasta un proceso que duró menos de un segundo recibe su nombre, y el filtro de un proceso
  conserva exactamente sus paquetes. Si no, una conversación pertenece al proceso cuyo socket la
  tenía durante la captura, y el resto va a **Proceso desconocido**.
- **2 Hosts** muestra las demás máquinas con los protocolos vistos. Los nombres salen de las
  respuestas DNS de la propia captura y de los nombres de servidor que pidieron las conexiones TLS,
  así que la mayoría de los hosts tienen uno.
- **3 Conexiones** muestra cada conversación con su proceso, el otro extremo, el protocolo, los
  paquetes y los bytes. Los detalles muestran ambos extremos, el nombre del servidor, el servicio,
  los bytes enviados y recibidos, y el flujo de la conversación: lo que envió cada lado, en orden y
  con los bytes retransmitidos contados una sola vez, en un volcado hexadecimal con los bytes que se
  imprimen como texto al lado. El flujo se divide en turnos, cada uno con una flecha que indica su
  sentido, los bytes que contiene y cuándo llegó. Cuando a la captura le faltan bytes antes de un
  turno, ese turno dice cuántos faltan. Cuando la captura guardó solo el principio de la conexión,
  el encabezado del flujo dice cuántos bytes dejó fuera después, según las cabeceras de los
  paquetes. **]** y **[** saltan al turno siguiente y al anterior. F3 muestra el flujo entero como
  filas hexadecimales en el visor, con sus turnos uno tras otro, para recorrerlo y buscar en él.
  Enter muestra los paquetes de la conversación uno a uno. Cuando Borg tiene las claves TLS de la
  conexión, el flujo muestra lo que llevaban sus registros cifrados (consulta [Descifrar
  TLS](#decrypting-tls)).
- **4 Paquetes** muestra todos los paquetes en orden con su tiempo desde el inicio, protocolo,
  longitud y un resumen como `ClientHello example.com` o `A example.com → 203.0.113.10`. Los
  detalles muestran sus capas (Ethernet, IP, TCP o UDP, y DNS, TLS o HTTP cuando Borg los lee) y sus
  bytes en un volcado hexadecimal. Un volcado pone 16 bytes en cada línea, y menos en un panel
  estrecho, para que el texto siga cabiendo al lado. F3 muestra los bytes del paquete como filas
  hexadecimales en el visor.

| Tecla | Acción |
| --- | --- |
| 1 / 2 / 3 / 4 | Mostrar Procesos / Hosts / Conexiones / Paquetes |
| Enter | Bajar un nivel: de un proceso o host a sus conexiones, de una conexión a sus paquetes, de un paquete a sus detalles |
| ] / [ | En los detalles de una conexión, saltar al turno siguiente o anterior del flujo |
| `/` | Filtrar las filas, como en las páginas en vivo |
| Ctrl-C | Copiar el PID, la dirección, el otro extremo o los bytes del paquete en hex |
| F3 | Mostrar el flujo de la conexión o los bytes del paquete como filas hexadecimales en el visor |
| Tab | Pasar entre la lista y los detalles, donde ↑↓ desplazan |
| Esc | Volver a la lista, luego subir un nivel, luego borrar el filtro y luego volver a la pantalla en vivo |

El filtro acepta las mismas palabras que las páginas en vivo, y `proto:` además coincide con lo que
Borg reconoció en los paquetes: `proto:dns`, `proto:tls`, `proto:http` y `proto:quic`. Un filtro se
aplica a todas las páginas, y Enter pone uno para bajar un nivel, por eso Esc puede volver atrás.

F3 sobre un archivo `.pcap`, `.pcapng` o `.cap` en un panel lo abre en el mismo visor, empezando
por **2 Hosts**. Un archivo grabado en otro sitio no tiene sockets con los que emparejarse, así que
sus conversaciones muestran **Proceso desconocido**. Un archivo `.pcapng` cuyos paquetes llevan su
proceso, como los que guardan Borg y `tcpdump -i pktap` en macOS, sigue nombrando cada proceso. El
visor lee archivos de hasta 256 MiB y sus primeros 200.000 paquetes.

Cuando Borg no encuentra tcpdump, la captura ofrece instalarlo. El diálogo **Instalar tcpdump**
muestra el comando exacto del gestor de paquetes que encontró Borg: `apt-get`, `dnf`, `pacman`,
`zypper` o `apk` en Linux, o el `brew` de Homebrew en macOS y en un Linux sin ninguno de ellos. Los
gestores de paquetes del sistema se ejecutan con `sudo`, o con `doas` donde no hay `sudo`, salvo
que Borg se ejecute como root. Enter escribe el comando en el shell de Borg, donde ves lo que hace
y escribes tú mismo la contraseña, y Esc cancela. Cuando el comando termina, un aviso dice si
funcionó, y Ctrl-O muestra su salida. Borg vuelve a buscar tcpdump cada vez que empieza una
captura, en `PATH` y en `/usr/sbin`, `/usr/bin`, `/sbin`, `/bin` y las carpetas `sbin` de Homebrew,
así que R funciona en cuanto termina la instalación. Sin un gestor de paquetes que Borg conozca, la
captura termina con **Para capturar hace falta tcpdump, y Borg no lo encuentra.**

tcpdump necesita acceso al dispositivo de captura. Borg lo ejecuta como tú, así que date acceso una
vez:

- En macOS, al principio solo root puede abrir los dispositivos `/dev/bpf`. Instala una vez
  Wireshark con su componente **ChmodBPF**: entrega los dispositivos a un grupo `access_bpf` y te
  añade a él, y a partir de entonces puedes capturar en todas las interfaces.
- En Linux, da una vez a tcpdump las capacidades de captura con
  `sudo setcap cap_net_raw,cap_net_admin=eip /usr/sbin/tcpdump`. Algunas distribuciones permiten
  capturar a los miembros de un grupo `pcap` o `wireshark`.

Sin acceso, Borg pregunta **Capturar como administrador** donde el sistema puede pedir tu
contraseña: en macOS en el diálogo de contraseña del propio sistema, y en Linux a través de
`pkexec`, cuyo diálogo viene de tu escritorio. Elige Sí y escribe la contraseña en el diálogo del
sistema, que Borg nunca ve. Mientras el diálogo está abierto, el pie dice **Esperando tu
contraseña para capturar en** y el nombre de la interfaz. Después Borg ejecuta una copia de sí
mismo como root, que comprueba qué se le pidió capturar e inicia tcpdump desde las carpetas del
propio sistema. Los paquetes vuelven a Borg, que guarda el archivo como tú. **R** y la duración
detienen la captura como antes, y si Borg termina, la captura se detiene con él. Si cancelas el
diálogo del sistema, la captura termina sin aviso. Para saltarte la pregunta, elige
**Administrador** en **Ejecutar como** antes de empezar.

En Linux sin `pkexec`, Borg usa `sudo` cuando puede ejecutar tcpdump sin contraseña. Por SSH no hay
escritorio que muestre el diálogo, así que la captura termina con **Borg no puede capturar como
administrador** y el motivo del sistema. Cuando el propio Borg se ejecuta como root, nunca
pregunta, y en macOS sus capturas siempre llevan el proceso de cada paquete. Donde Borg no puede
preguntar, la captura termina con **No tienes permiso para capturar en** y el nombre de la
interfaz.

### Descifrar TLS

Cuando Borg tiene las claves de una conexión TLS, su flujo en **3 Conexiones** muestra lo que
llevaban los registros cifrados, como una petición HTTP y su respuesta, y el encabezado del flujo
termina en **descifrado**.

Las claves salen de un archivo de claves TLS, el que escriben los navegadores, curl, Python y muchos
otros programas cuando la variable de entorno `SSLKEYLOGFILE` nombra uno. Entonces nada se
interpone entre el programa y el servidor, así que esto también funciona con programas que fijan
sus certificados. Para los programas que nunca escriben un archivo de claves, Borg puede ponerse él
mismo en medio (consulta [Por el proxy de Borg](#through-borgs-proxy)).

Para guardarlas con una captura, pon **Claves TLS** en **Definir SSLKEYLOGFILE en el shell de
Borg** en el diálogo **Capturar tráfico**. Cuando la captura empieza, Borg crea
`capture-<date>-<time>.keylog` junto a la captura, legible solo por ti, y escribe una línea como
esta en el prompt de su shell, donde la ves ejecutarse y la encuentras después en el historial del
shell:

```sh
export SSLKEYLOGFILE='/home/me/projects/capture-20260101-120000.keylog'
```

En fish la línea empieza con `set -gx`. Después Borg vuelve a la pantalla de red, donde el pie dice
**Claves TLS** mientras la captura se ejecuta.

Ahora inicia el programa desde la línea de comandos de Borg, o pulsa Ctrl-O e inícialo en el shell.
Hereda la variable y escribe sus claves en el archivo. Para un programa en otra terminal, pulsa
**L** en la pantalla de red para copiar `SSLKEYLOGFILE='…'` con la ruta del archivo de claves,
pégalo allí y escribe el programa detrás. Un programa que ya estaba en marcha conserva el ajuste con
el que empezó, así que ciérralo e inícialo de nuevo. Eso vale sobre todo para los navegadores, que
pasan un segundo inicio a la ventana que ya está abierta. En macOS, `open` inicia las aplicaciones
sin las variables del shell, así que inicia el propio programa de la aplicación, como
`/Applications/Firefox.app/Contents/MacOS/firefox`.

Si el shell de Borg está ocupado, o la línea de comandos pertenece a un contenedor o a otra máquina,
Borg no escribe nada y un aviso lo dice. **L** sigue copiando la línea.

Cuando la captura termina, Borg escribe `unset SSLKEYLOGFILE` en el prompt (`set -e SSLKEYLOGFILE`
en fish), o restablece el valor con el que arrancó el propio Borg, y un aviso nombra la línea. Si un
programa que iniciaste sigue ejecutándose en el shell, la línea espera a que termine. Luego Borg pone
las claves de las conexiones de la captura dentro de la captura como un bloque de secretos de
descifrado, así que un `.pcap` pasa a ser un `.pcapng` con el mismo nombre, que Wireshark abre ya
descifrado. El archivo de claves se queda a su lado, porque los programas iniciados durante la
captura siguen escribiendo en él hasta que terminan. Las capturas que pide un agente no guardan
claves.

Leen `SSLKEYLOGFILE` Firefox, Chrome y otros navegadores basados en Chromium, la mayoría de las
compilaciones de curl y los programas de Python que usan el contexto predeterminado del módulo
`ssl`. Otros necesitan su propia opción:

- Node.js escribe sus claves con `--tls-keylog`, así que inícialo como
  `node --tls-keylog="$SSLKEYLOGFILE" app.js`.
- Los programas de Go las escriben solo cuando el programa define `KeyLogWriter` en su configuración
  TLS.
- Los programas de Rust con rustls las escriben solo cuando usan su `KeyLogFile`.
- Java necesita un agente que se enganche a su código TLS.

Safari y otras aplicaciones creadas sobre las bibliotecas TLS propias de Apple nunca escriben un
archivo de claves, y su tráfico sigue cifrado.

#### Por el proxy de Borg

Pon **Claves TLS** en **Pasar el HTTPS por el proxy de Borg con una CA temporal** para leer el
HTTPS de programas que nunca escriben un archivo de claves, como programas en Go y Rust o un
cliente que no puedes cambiar. Cuando empieza la captura, Borg ejecuta un proxy en `127.0.0.1` y
crea una autoridad de certificación (CA) solo para esta captura. La clave de la CA vive en la
memoria de Borg y desaparece cuando termina la captura. Borg escribe una línea en el prompt de su
shell que envía el HTTPS por el proxy y hace que los programas confíen en la CA:

```sh
export HTTPS_PROXY='http://127.0.0.1:54321' https_proxy='http://127.0.0.1:54321' SSL_CERT_FILE='…' CURL_CA_BUNDLE='…' REQUESTS_CA_BUNDLE='…' NODE_EXTRA_CA_CERTS='…'
```

Las cuatro variables de certificados nombran un único archivo en la carpeta de ejecución de Borg,
que solo tú puedes leer. Ese archivo tiene el certificado de la CA seguido de los certificados del
propio sistema, así que los programas siguen confiando en todos los demás hosts. Mientras la
captura se ejecuta, el pie muestra **Proxy en** y la dirección del proxy, y **L Copiar ajustes del
proxy** copia las mismas variables para un programa en otro terminal. Cuando la captura termina,
Borg le devuelve al shell lo que tenía, igual que con `SSLKEYLOGFILE`, y borra el archivo. Borg
nunca añade su CA al almacén de certificados de confianza del sistema.

Un programa que pasa por el proxy le pide un túnel hasta el host. El proxy se conecta al host,
comprueba su certificado como lo hace el sistema y luego le muestra al programa un certificado
para el mismo host firmado por la CA. El programa y el host siguen hablando HTTP/2 o HTTP/1.1,
según lo que eligió el host. El proxy escribe las claves de las dos sesiones TLS en el archivo de
claves de la captura, así que la copia de la conexión con el host que hay en la captura se abre
descifrada, y el visor se la asigna al proceso que pidió el túnel. Todo lo que pasa por un túnel
sin ser TLS sigue tal cual.

Los programas pasan por el proxy cuando leen los dos tipos de variables:

- curl, git y Python con requests o urllib siguen `HTTPS_PROXY` o `https_proxy`, y confían en el
  archivo de certificados a través de `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, o `SSL_CERT_FILE`
  cuando usan OpenSSL.
- Go sigue `HTTPS_PROXY`, salvo en las peticiones a `localhost`, y lee `SSL_CERT_FILE` en Linux. En
  macOS solo confía en los certificados del sistema, así que allí sus conexiones fallan.
- Node.js confía en la CA a través de `NODE_EXTRA_CA_CERTS`. Las versiones recientes envían `fetch`
  y `https` por el proxy cuando además defines `NODE_USE_ENV_PROXY=1`.
- Los programas con su propia lista de certificados, los que fijan certificados y las
  aplicaciones que toman el proxy de los ajustes del sistema, como Safari, rechazan o esquivan el
  proxy. Un rechazo aparece como **Un programa no confió en el certificado del proxy para** y el
  host.

Cuando el certificado del host no pasa la comprobación, el proxy cierra la conexión y un aviso dice
**Borg no pudo verificar** y el host. Es lo que pasa con tus propios servicios cuando sus
certificados vienen de una CA de desarrollo en la que el sistema no confía, como la que crea
[mkcert](https://github.com/FiloSottile/mkcert). Apunta `proxy-ca`, en la sección `[network]` de
`config.toml`, a la carpeta de esa CA:

```toml
[network]
proxy-ca = "~/Library/Application Support/mkcert"  # mkcert -CAROOT prints the folder
```

La carpeta debe contener el certificado y su clave como `rootCA.pem` y `rootCA-key.pem`, los
nombres que les da mkcert, o como `ca.pem` y `ca-key.pem`. La opción del diálogo pasa a ser
**Pasar el HTTPS por el proxy de Borg con la CA de** y la carpeta. El proxy firma con tu CA, en la
que tus programas ya confían, y también confía en ella cuando comprueba tus servicios. Usa una CA
que exista solo para desarrollo, porque quien tenga su clave puede leer el tráfico de todo lo que
confía en ella.

F3 busca claves en dos sitios:

- Un bloque de secretos de descifrado dentro de un archivo `.pcapng`, donde Wireshark también las
  guarda.
- Un archivo de claves junto a la captura, con el mismo nombre y la extensión `.keylog`, como
  `capture-20260101-120000.keylog` junto a `capture-20260101-120000.pcap`.

Para descifrar una captura que hiciste de otra forma, ejecuta el programa con la variable definida
durante la captura y, cuando la captura se haya guardado, cambia el nombre del archivo de claves
para que coincida con el de la captura:

```sh
SSLKEYLOGFILE=~/tls.keylog curl https://example.com
```

Borg abre registros de TLS 1.3 y TLS 1.2 cifrados con AES-128-GCM, AES-256-GCM o
ChaCha20-Poly1305, los cifrados que acuerdan los navegadores y los servidores actuales. Un registro
que ninguna clave abre conserva sus bytes cifrados, y su turno dice **aún cifrado**. Eso pasa cuando
al archivo de claves le faltan algunas claves de la conexión, y después de bytes que faltan en la
captura, porque Borg no puede saber dónde empieza el registro siguiente. El tráfico QUIC sigue
cifrado.

Un archivo de claves descifra todas las conexiones de las que tiene claves, con contraseñas, cookies
y tokens incluidos, y lo mismo hace una captura con las claves dentro. Borg hace que solo tú puedas
leer ambos. Haz lo mismo con un archivo de claves que escribas tú con `chmod 600`, guarda los
archivos de claves con el mismo cuidado que las capturas y borra ambos cuando termines.

### Lo que informa el sistema

En macOS, Borg lee cada socket con sus recuentos de bytes y su proceso desde `netstat`, así que el
tráfico aparece tanto para TCP como para UDP. También se listan los sockets de los procesos de otros
usuarios.

En Linux, Borg lee los sockets de `/proc/net` y encuentra el proceso de cada socket a través de los
archivos abiertos de los procesos. Sin derechos de administrador solo puede mirar dentro de tus
propios procesos, así que los sockets de otros usuarios muestran **Proceso desconocido**. Los
recuentos de bytes de TCP vienen de `ss` cuando está instalado; los sockets UDP y los equipos sin
`ss` muestran **—** en las velocidades, y los detalles dicen **El sistema no informa recuentos de
bytes aquí**. Los totales y las velocidades de las interfaces funcionan siempre.

## Elementos de inicio

**Alt-U** o **Comando → Elementos de inicio** lista todo lo que está configurado para arrancar solo:
al iniciar el sistema, al iniciar sesión, según un horario, o cada vez que arranca un shell o una
sesión SSH. El cambio más reciente va primero, así que lo que se añadió o editó hace poco queda
arriba. Igual que el monitor de actividad, viene del complemento Sistema incluido. La lista ocupa el
panel activo, y el otro panel muestra el elemento seleccionado en detalle.

Cada fila muestra cuándo cambió su archivo por última vez, su lugar y lo que ejecuta. Un panel más
ancho añade el propietario del archivo y si su programa está ahí: **existe**, o **falta** cuando la
ruta que indica no existe. Un programa que falta merece una mirada, porque cualquier archivo que
alguien ponga ahí más tarde se ejecuta con los derechos del elemento. La columna **Reglas** nombra
las reglas con las que coincidió el elemento, coloreadas según su nivel, y aparece en cuanto algún
elemento coincide con una. La línea sobre la lista cuenta los elementos y cuántos coincidieron con
una regla.

Los detalles muestran el archivo y su línea, lo que ejecuta el elemento, el programa y dónde lo
encontró Borg, cuándo se ejecuta con las palabras del propio archivo (`RunAtLoad`, `@reboot`, un
calendario o un objetivo de systemd), el usuario con el que se ejecuta, el propietario del archivo y
la hora exacta del cambio. En **Reglas**, cada coincidencia dice si fue **en su archivo** o
**en su programa**, con lo que busca la regla, cuándo coincide con cosas inofensivas y de dónde
viene la regla. Un elemento que su propio archivo desactiva aparece atenuado, y sus detalles dicen
**Desactivado en su propio archivo, así que no arranca.**

| Tecla | Acción |
| --- | --- |
| Enter | Cerrar la pantalla y mostrar el archivo del elemento en el panel |
| F3 / F4 | Ver / editar el archivo del elemento |
| `/` | Filtrar las filas; ↑↓ siguen moviendo, Enter conserva el filtro, Esc lo borra |
| R | Volver a leer todos los lugares |
| Ctrl-C | Copiar lo que ejecuta el elemento |
| Alt-W | Maximizar la lista |
| Esc | Borrar el filtro y después cerrar la pantalla |

Haz doble clic en una fila para mostrar su archivo en el panel. El filtro conserva los elementos que
contienen todas las palabras que escribas, en el lugar, el nombre, lo que ejecuta, la ruta del
archivo, el propietario, el usuario con el que se ejecuta, cuándo se ejecuta, **existe** o
**falta**, y los nombres de las reglas con las que coincidió. Pon `!` delante de una palabra para
excluir lo que encuentre: `root !timer` conserva los elementos de root que no son timers. Cada lugar
tiene además un nombre corto para el filtro y para los agentes, como `launch-daemon`,
`systemd-timer` o `ssh-authorized-key`.

### Dónde busca Borg

En macOS:

- **Agente de usuario**: `~/Library/LaunchAgents`.
- **Agente launchd** y **Daemon launchd**: `/Library/LaunchAgents` y `/Library/LaunchDaemons`.
- **Agente del sistema** y **Daemon del sistema**: `/System/Library/LaunchAgents` y
  `/System/Library/LaunchDaemons`, en el volumen de sistema sellado.
- **Tarea cron**: los crontabs de `/usr/lib/cron/tabs`, y `/etc/crontab`.
- **Script periodic**: los scripts diarios, semanales y mensuales de `/etc/periodic` y
  `/usr/local/etc/periodic`.

En Linux:

- **Servicio** y **Timer**: unidades de systemd en `/etc/systemd/system`,
  `/usr/local/lib/systemd/system`, `/usr/lib/systemd/system` y `/lib/systemd/system`, y las unidades
  a las que apuntan sus carpetas `.wants` y `.requires` desde cualquier otro lugar.
- **Servicio de usuario** y **Timer de usuario**: `/etc/systemd/user`,
  `/usr/local/lib/systemd/user`, `/usr/lib/systemd/user`, `~/.config/systemd/user` y
  `~/.local/share/systemd/user`.
- **Inicio automático**: entradas de escritorio en `~/.config/autostart` y `/etc/xdg/autostart`.
- **Tarea cron**: `/etc/crontab`, `/etc/cron.d` y los crontabs de `/var/spool/cron/crontabs` o
  `/var/spool/cron`.
- **Script cron**: `/etc/cron.hourly`, `/etc/cron.daily`, `/etc/cron.weekly` y `/etc/cron.monthly`.
- **Tarea anacron**: `/etc/anacrontab`.
- **rc.local**: `/etc/rc.local`.
- **Precarga**: `/etc/ld.so.preload`, las bibliotecas que se cargan en cada programa que arranca.
- **Inicio del shell**: los scripts de `/etc/profile.d`, además de los archivos de abajo.

En ambos, **Inicio del shell** abarca los archivos de inicio de zsh, bash y fish de tu carpeta
personal, como `~/.zshrc`, `~/.bashrc`, `~/.profile` y `~/.config/fish/config.fish`, y sus versiones
para todo el sistema en `/etc`. **Clave SSH** lista cada clave de `~/.ssh/authorized_keys` y
`~/.ssh/authorized_keys2` con su comentario, y con el comando que impone cuando tiene uno.

Borg lee estos lugares con tus permisos. Un lugar que existe pero está cerrado para ti aparece al
final de la lista como **No se pudo leer** con su ruta; ejecuta Borg como administrador para ver
dentro. Tu propio crontab es la excepción: cuando la carpeta de crontabs está cerrada para ti, como
en macOS, Borg se lo pide a `crontab -l`.

Un programa indicado con una ruta completa o `~/` se marca como **falta** cuando ahí no hay nada. Un
nombre sin carpeta se busca en `/usr/local/sbin`, `/usr/local/bin`, `/usr/sbin`, `/usr/bin`, `/sbin`
y `/bin`, donde buscan cron y launchd. Una ruta relativa, o un nombre que no está en ninguna de esas
carpetas, muestra **—**, porque el elemento puede definir su propia ruta de búsqueda.

### Reglas en los elementos de inicio

Borg aplica sus propias reglas y las tuyas (consulta
[Analizar archivos con reglas](#scanning-files-with-rules)) al primer 1 MiB de cada archivo de
inicio y a cada programa de hasta 64 MiB. Un programa que ejecutan muchos elementos se analiza una
sola vez. Los programas de los elementos **Agente del sistema** y **Daemon del sistema** no se
analizan, porque nada puede cambiar el volumen de sistema sellado mientras macOS está en marcha, y
sus detalles lo indican. Cuando un archivo de reglas no compila, el final de la lista dice cuántos
quedaron fuera.
