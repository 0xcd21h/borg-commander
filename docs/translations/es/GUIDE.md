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
Enter pase a la siguiente. Los archivos Markdown y JSON se abren ahí como código fuente, porque la
búsqueda leyó el archivo tal cual, y `s` cambia a la vista formateada. Borg lee los archivos en
segundo plano, y la línea de contexto cuenta los archivos leídos. Los archivos binarios (un byte
NUL en los primeros 8 KiB) y los de más de 64 MiB quedan sin leer y cuentan como binarios o
demasiado grandes. Ctrl-C detiene la búsqueda, que se para en 20.000 archivos como cualquier
búsqueda.

### Trabajar con los resultados

Haz clic en el encabezado de una columna para ordenar por ella, y otra vez para invertir el orden.
Los menús Izquierda y Derecha también permiten ordenar, y el menú Buscar añade la ordenación por
carpeta de origen y la agrupación por carpeta o por tipo de archivo. Enter sobre el encabezado de
un grupo lo pliega; Insert marca los archivos coincidentes que contiene. Las marcas se conservan
al ordenar o filtrar, y el pie también cuenta los archivos marcados que el filtro oculta.

F3 y Ctrl-Q abren el archivo original. F5 copia a una carpeta real; si el otro panel es una
colección, F5 añade referencias a ella. F6 necesita una carpeta real como destino. En una
colección, F8 quita referencias y deja los archivos intactos; en los resultados de búsqueda, F8
pide confirmación para eliminar los archivos originales. Shift-F8 también elimina los archivos
originales desde una colección, tras confirmarlo. Si marcas una carpeta y archivos que contiene,
la tarea procesa la carpeta una sola vez. **Mostrar original** abre la carpeta del archivo en el
otro panel. Left o Esc ocultan los resultados, y Alt-Shift-R los recupera tal como los dejaste.
Tab cambia de panel incluso si el shell tiene un comando a medio escribir. El shell siempre
permanece en una carpeta real. F10 siempre pregunta antes de salir.

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
| Tab | Cambiar de panel, o entrar en la vista previa rápida y volver a la lista de archivos |
| Ctrl-O | Mostrar el shell / volver a los paneles |
| Alt-I | Contar tamaños de carpetas en segundo plano / dejar de contar |
| Ctrl-S | Búsqueda rápida en la carpeta |
| Ctrl-Q | Vista rápida: previsualiza en el otro panel el archivo bajo el cursor (texto, imagen o carpeta) |
| Ctrl-L | Información: muestra en el otro panel el tamaño, los permisos, el propietario, las fechas y el espacio libre |
| Insert, Ctrl-T, Shift-Up/Down | Marcar archivos |
| `+` `-` `*` | Seleccionar, deseleccionar o invertir por patrón (`*.rs`, `build/`) |
| Alt-Enter | Poner los nombres marcados (o el actual) en la línea de comandos |
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
| Alt-J | Tareas: pausar o cancelar copias, movimientos y eliminaciones en curso |
| Alt-M | Monitor de actividad: procesos y memoria |
| Alt-C | Docker: proyectos de Compose y contenedores |
| Alt-T | Time Machine: qué dejan fuera las copias de seguridad, en macOS |
| Alt-E | Editar el archivo seleccionado en este panel, en lugar de sus archivos |
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
contenedores, y mostrar un archivo o una carpeta en tu otro panel.

Todo lo que modifica la máquina espera tu respuesta en un diálogo que muestra el cambio completo:
un comando y dónde se ejecutará, un archivo nuevo o una edición en forma de diff. Desplaza los
largos con las flechas, PgUp y PgDn, o la rueda del ratón, y pulsa F3 para ver el diálogo a
pantalla completa. La opción No sigue seleccionada hasta que eliges Sí, y una edición se rechaza si el
archivo cambió mientras decidías. Con una computadora emparejada, `approval_mode` te pregunta una
vez antes de activar Aprobar todo para esa sesión. Mientras está activo, se mantiene visible una
insignia; haz clic en ella o pulsa Ctrl+Alt+A para desactivarlo al instante. También se desactiva
cuando se cierra esa conexión emparejada. Las aprobaciones de emparejamiento son aparte.

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
búsqueda. Una vez que lo apruebas, el agente puede volver a buscar en ese proceso hasta que borg
sale. Puede pedir leer como administrador, lo que abre la petición de contraseña del propio
sistema.

Mientras hay un agente conectado, **◈ Remoto** aparece al principio de la barra de atajos. Haz
clic en él para ver un registro de lo que ha hecho el agente: lo que ha mirado, leído, mostrado y
escrito, y cada comando y cambio, aprobado o rechazado. Borg guarda el registro solo en memoria,
hasta que sale; para no guardar ningún registro, desactívalo en Opciones → Complementos.

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
   código. Elige Sí si los códigos coinciden. Después, `borg pair` ofrece añadir el servidor a
   Claude Code, como `claude mcp add borg-NAME -- borg mcp remote NAME`.

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

La conexión está cifrada de extremo a extremo. Cada borg guarda una clave privada, que solo tú
puedes leer, en su carpeta de estado. El emparejamiento intercambia las claves públicas, y cada
conexión empieza con un intercambio Noise IK ligado a ellas, de modo que el relay solo reenvía
texto cifrado y no puede responder en nombre de ninguno de los dos borgs. Las llamadas desde otra
computadora pueden leer sin preguntar, igual que las locales; activa "preguntar antes de que otra
computadora lea" en Opciones → Complementos para que esas lecturas también esperen aprobación.
Borg no mantiene ninguna conexión con el relay salvo que esté emparejado o mostrando un enlace de
emparejamiento, y ninguna en absoluto cuando borg cloud está desactivado en Opciones →
Complementos.

Borg guarda el relay en la configuración como `relay = "wss://relay.example.com"`. Una computadora
llega al servidor a través del relay con el que se emparejó, así que, si el servidor cambia de
relay, vuelve a emparejar la computadora. Todos los borgs de una misma máquina comparten una clave,
así que solo el primero que se abre se conecta al relay; los demás te indican cuál tiene borg
cloud.

### Sesiones con nombre y el servicio en segundo plano

Con espacios de trabajo con nombre, `borg sessions list` muestra sus ID y nombres,
`borg sessions create NAME` añade uno y `borg --session NAME` lo abre. Por MCP, el agente puede
llamar a `list_sessions`, `list_remote_sessions` y `use_session` para elegir con qué sesión trabaja
esa conexión. Una sola llamada a una herramienta también puede pasar un ID de `session` para
dirigirse a otra sesión solo esa vez. La sesión elegida solo se aplica a esa conexión MCP.

`borg service install`, `enable` y `start` configuran el servicio opcional en segundo plano. Este
mantiene la conexión de emparejamiento de la máquina y deja las sesiones con nombre en marcha
después de que se cierren sus terminales. `borg service attach NAME` vuelve a conectarse a una, y
Ctrl-] se desconecta de nuevo sin terminar su shell. Usa `borg service stop`, `disable` o
`uninstall` para detener o quitar el servicio. En un contenedor sin systemd, haz que el supervisor
de procesos del contenedor ejecute `borg service run`; los comandos `install` y `enable` necesitan
un host Linux con systemd.

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

### Hosts remotos

En el Borg local, pulsa **Alt-H**, haz clic en **Hosts remotos** en la barra de atajos o elige
**F9 → Remoto → Explorar hosts y sesiones…** para ver las máquinas emparejadas y sus sesiones con
nombre. Enter abre los archivos de la sesión seleccionada en el panel desde el que abriste el
selector, y sustituye lo que mostraba ese panel. Pulsa **M** para reflejar la pantalla de la sesión
y **R** para actualizar la lista.

**Tab** alterna entre **Hosts** (computadoras a las que este Borg puede llegar) y
**Clientes** (computadoras que pueden llegar a este Borg). Clientes enumera las computadoras
emparejadas con este Borg, las huellas de sus claves públicas, si están conectadas y cuántas
conexiones tiene cada una. Una computadora aparece una sola vez aunque un espejo use varias
conexiones. El estado se actualiza mientras el selector está abierto, también en una sesión cuyas
conexiones mantiene el servicio en segundo plano. El emparejamiento funciona en un solo sentido: un
cliente autorizado no se convierte automáticamente en un host cuyos archivos pueda explorar este
Borg. Cuando el propietario de la conexión no puede informar del estado de la conexión, los
clientes emparejados siguen apareciendo, con **Estado de conexión no disponible**. Un mismo Borg
puede ser host y cliente a la vez. El menú Remoto también ofrece **Reflejar host montado** y
**Compartir este Borg como host…**.

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
recupera el control del espejo. El espejo sigue la disposición del terminal del host y se recorta
para caber en una ventana más pequeña; si agrandas la ventana, ves el resto. El servicio opcional
en segundo plano mantiene las sesiones vivas después de que se cierre su terminal original y
permite que el cliente que tiene el control cambie el tamaño de su PTY.

Si otro terminal tiene el control de la sesión remota, el espejo se abre en modo de solo lectura y
tus teclas no llegan a la sesión. Pulsa **Ctrl-T** o haz clic en la línea de estado para tomar el
control y desconectar el otro terminal, o empieza con `borg mirror MACHINE:SESSION --take`;
`--take` siempre toma el control de otro espectador. Una vez que tienes el control, la primera
tecla activa una pantalla remota en reposo, y las teclas y la entrada del ratón posteriores llegan
a ella. Un espectador que se queda atrás omite los fotogramas antiguos de
animación y pasa directamente a la pantalla actual.

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

### Visor y vista rápida

F3 abre el visor. En los archivos de texto, las flechas y PgUp/PgDn desplazan, `w` activa o
desactiva el ajuste de línea, y Esc o `q` lo cierran. En las imágenes, Left/Right o `p`/`n`
muestran la imagen anterior o la siguiente. Borg usa el protocolo de imágenes del terminal cuando
lo hay; los demás terminales muestran la imagen como bloques de colores.

Ctrl-Q muestra una vista previa rápida en el otro panel. Tab entra en ella, donde las flechas,
PgUp/PgDn, Home/End y la rueda del ratón desplazan; Tab o Esc vuelven a la lista de archivos.
Ctrl-L muestra la información del archivo. F3 abre una vista previa de texto en el visor completo,
y al cerrar el visor se vuelve a la vista previa conservando la posición, el ajuste de línea y el
modo de código fuente. Ambos leen los archivos de la misma forma limitada, así que abrir un archivo
de texto grande solo lee aproximadamente la primera página y es rápido sea cual sea su tamaño.

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
archivos, para que puedas editar dos archivos uno al lado del otro, o un archivo junto a la
carpeta en la que está. F6 pasa del editor de un panel al otro panel, y Tab vuelve al editor desde
la lista de archivos. El texto copiado con Ctrl-C en un editor se pega con Ctrl-V en el otro.
Mientras un editor tiene el foco, la barra de atajos muestra las teclas del editor, y las teclas
del panel vuelven con la lista de archivos. Ctrl-O también muestra la consola desde un editor, y
F9 abre los menús. Esc o F10 cierra el editor y devuelve al panel sus archivos; al salir, Borg
pregunta primero por cada editor de panel con cambios sin guardar. La vista rápida y la
información del archivo necesitan el otro panel, así que esperan hasta que se cierre su editor.

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

En un terminal local con color verdadero, Borg arranca con una breve animación de su logotipo con
efecto glitch; cualquier tecla o clic la omite. Por SSH, y en terminales sin color verdadero, se
abre directamente en los paneles.

Las carpetas de los paneles, las posiciones del cursor, la división, los modos de listado y la
ordenación se guardan en `~/.local/state/borg-commander`, o en la carpeta indicada por
`$XDG_STATE_HOME` o `$BORG_STATE_DIR`.

Mientras se ejecuta, Borg guarda sus scripts de integración con el shell y los archivos copiados
desde contenedores en `$XDG_RUNTIME_DIR/borg-commander`, o en `borg-commander-<uid>` dentro de la
carpeta temporal. Solo tú puedes abrir esta carpeta; si ya existe una creada por otra persona, o
en la que pueden entrar otros usuarios, Borg la ignora y usa otro nombre. Su archivo `TTL` empieza
con la hora hasta la que la carpeta está en uso: cada borg en ejecución adelanta esa hora un día
cada hora, así que, una vez pasada, la carpeta se puede eliminar.

Los temas personalizados van en `~/.config/borg-commander/themes/`. Un tema puede extender uno de
los integrados:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

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
| Space | Pausar o reanudar el muestreo |
| R | Actualizar la vista actual |
| Esc | Volver; en la lista de procesos, cerrar el monitor |

Haz clic en el encabezado de una columna para ordenar, y doble clic en un proceso para abrir su
mapa de memoria. El resumen muestra la memoria residente actual, el valor más alto observado, el
cambio desde que empezó la monitorización, la CPU, el espacio de direcciones virtual y un gráfico
de la memoria residente de los últimos 60 segundos. Los totales de memoria usada, en swap, privada
y compartida aparecen una vez leído el mapa de memoria, si el sistema los proporciona. **Enter** en
el panel de memoria abre el mapa de memoria. El mapa de memoria se lee cuando lo pides; pulsa R
para volver a leerlo después de que el proceso cambie sus asignaciones.

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

### Acceso de administrador

Cuando la lectura de un mapa de memoria o de una captura está restringida, o una búsqueda no pudo
leer parte de la memoria, Borg ofrece **A Administrador**. En macOS, esto abre la petición de
autorización del sistema; en Linux usa `pkexec` y necesita que haya un agente de autorización en
ejecución. Solo un asistente de inspección de corta duración se ejecuta con privilegios elevados.
Borg sigue ejecutándose con normalidad, y el asistente solo devuelve datos del mapa o una cantidad
limitada de bytes.

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
