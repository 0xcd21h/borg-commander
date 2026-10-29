# Borg guía de usuario

[Volver al escaparate](../../../README.md)

[Instalar](#install) · [Temas e íconos](#themes-and-icons) · [llaves](#keys) ·
[Ratón y consola](#mouse-and-console) · [complemento de git](#git-plugin) ·
[Buscar, trabajos y espectadores](#find-jobs-and-viewers) · [Configuración](#configuration)

## Instalar

Descarga un binario desde [GitHub Releases](https://github.com/0xcd21h/borg-commander/releases) o ejecuta el instalador en macOS o Linux, con procesador Intel o ARM:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install.sh | sh
```

El instalador verifica la suma de comprobación del archivo e instala `borg` en un directorio del PATH, para ejecutarlo en el mismo terminal: utiliza `~/.local/bin` si ya está en el PATH; `/usr/local/bin` si se puede escribir allí, por ejemplo como root en un contenedor; o, en caso contrario, `~/.local/bin`, que añade al archivo de inicio de la shell. Después inicia Borg. `BORG_NO_RUN=1` omite ese inicio y `BORG_INSTALL_DIR` permite elegir el directorio. Si esa versión ya está instalada, una nueva ejecución omite la descarga. Para compilar desde el código fuente, utiliza Rust 1.97 o posterior:

```sh
cargo install --locked --git https://github.com/0xcd21h/borg-commander borg-app
```

El soporte para Homebrew está preparado. Cuando se publique el repositorio tap:

```sh
brew install 0xcd21h/tap/borg-commander
```

Ejecuta `borg` en la carpeta que quieras explorar. F10 sale; Ctrl-O vuelve de la consola a los paneles. Ejecuta `borg update` para actualizar una instalación descargada, o utiliza el gestor de paquetes si instalaste mediante Homebrew o cargo. `borg --version` muestra la versión instalada.

## Temas e íconos

Elija un tema con F9 → Opciones → Tema. Ver el [galería temática](../../../README.md#themes) para Midnight, Aurora, Neon, Classic, Paper y Ember.

Los temas utilizan colores fijos para que las personalizaciones de la paleta del terminal no cambien su apariencia. Los terminales sin truecolor utilizan los colores más cercanos en la paleta 256-colour.

Los iconos de archivos siempre se muestran. Ghostty usa los íconos Nerd Font más ricos automáticamente. Otros terminales utilizan por defecto símbolos portátiles: `/` para carpetas, `@` para enlaces y `*` para ejecutables. conjunto `icon-style = "nerd"` utilizar iconos enriquecidos en otro terminal con una fuente adecuada, o `icon-style = "portable"` para forzar símbolos portátiles. El valor predeterminado, `"auto"`, se adapta al terminal sin guardar esa detección como preferencia de fuente.

Los archivos de lanzamiento incluyen **Borg Mono**, una fuente monoespaciada con íconos Nerd Font. Instale las fuentes normales y en negrita de `fonts/`, selecciona Borg Mono en tu terminal, y elige el `"nerd"` estilo de icono. Borg no puede cambiar la fuente de su terminal. El instalador los guarda en `${XDG_DATA_HOME:-~/.local/share}/borg-commander/fonts`; Homebrew los mantiene dentro `$(brew --prefix)/share/borg-commander/fonts`.

## Colecciones y búsqueda recursiva

El menú Buscar abre colecciones y búsqueda recursiva de nombres de archivos. Ambos utilizan resultados ordenables con rutas de origen y el visor existente y una vista previa rápida.

- **Ctrl-F:** busca de forma recursiva bajo la carpeta actual. Todas las palabras deben coincidir: `main rs` encuentra `main.rs`, y `a | b` encuentra cualquiera de las dos. Una palabra en minúsculas ignora mayúsculas y minúsculas; una con alguna mayúscula las respeta. También funcionan los globs como `*.log`, las frases entre comillas, las rutas con barra como `src/lib` y `-word` para excluir coincidencias, así como los filtros `ext:rs,toml`, `type:file`, `type:dir`, `type:link`, `size:>10M`, `size:1M..50M`, `mod:<7d` (modificado en la última semana), `mod:>1y` e `in:src` (una carpeta en cualquier parte de la ruta). Ctrl-P y Ctrl-N en el campo de consulta recuperan búsquedas anteriores, primero las hechas en la misma carpeta.
- **Alt-K:** recopile los archivos marcados, o el archivo actual, en el otro panel. Agregar una referencia no copia el contenido del archivo.
- **Alt-V:** elija una colección guardada de una lista desplazable con recuentos de entradas y estimaciones de tamaño. Presione F7 o N para crear uno. Las colecciones sobreviven al reiniciarse Borg.
- **Esc:** ocultar resultados, conservando el cursor, desplazamiento, marcas, filtro y agrupación.
- **Alt-Shift-R:** reanudar la vista oculta. El escaneo continúa mientras está oculto.
- **Alt-D — Tamaños de carpeta:** busque carpetas por su tamaño total o número de archivos, incluidos los archivos ocultos de forma predeterminada. Abre el cuadro de diálogo Ctrl-F configurado en *Totales de carpeta*; cambiando   *buscar* se mueve entre los dos. Ctrl-C detiene un escaneo.
- **Alt-S:** calcular los tamaños para la carpeta seleccionada. **Alt-Shift-D:** abra un árbol de tamaños para la carpeta actual.
- **Alt-W:** maximizar o restaurar el panel de resultados activo. Los resultados se abren en un panel normal de forma predeterminada. La barra de título `[+]` (maximizar), `[=]` (restaurar), y `[-]` Se puede hacer clic en los controles (plegar). Plegar oculta la vista; Alt-Shift-R lo reanuda con su diseño y selección conservados.
- **Ctrl-S:** filtrar los resultados por nombre o ruta de origen. Un filtro vacío lo muestra todo.

Haga clic en el encabezado de una columna para ordenarla o invertirla. Los menús Izquierda/Derecha también ofrecen clasificación; La búsqueda agrega clasificación y agrupación de carpetas de origen por carpeta o tipo de archivo. Enter en el encabezado de un grupo lo dobla; Insertar marca sus miembros coincidentes. Las marcas sobreviven a la clasificación y al filtrado; el pie de página también cuenta los archivos marcados ocultos por el filtro.

F3 y Ctrl-Q abren la fuente real. F5 copias a un destino real; si el panel de destino es una colección, agrega referencias en su lugar. F6 requiere un destino real. F8 elimina referencias de una colección. En los resultados de búsqueda, F8 solicita eliminar los originales. Shift-F8 también elimina los originales de una colección, después de la confirmación. Cuando se marcan tanto una carpeta como su contenido, la carpeta se envía al trabajo una vez. Revelar original abre su carpeta de origen en el otro panel. Izquierda o Esc oculta los resultados; Alt-Shift-R los retoma en el mismo lugar. Tab cambia los paneles virtuales incluso cuando el shell tiene un comando pendiente. El shell mantiene un directorio de trabajo real. F10 siempre pregunta antes de Salir.

La búsqueda lee nombres de archivos y metadatos, con un trabajador cancelable y un máximo de 20.000 coincidencias por búsqueda. La línea de contexto muestra la finalización, los errores, las entradas omitidas y si se alcanzó ese límite. Las carpetas ocultas siguen a Mostrar archivos ocultos. La búsqueda permanece en el sistema de archivos inicial y no se repite a través de enlaces simbólicos del directorio. La ventana de búsqueda puede restringir los resultados a archivos o carpetas y establecer un tamaño mínimo. elegir **Totales de carpeta** para medir todos los archivos descendientes antes de filtrar por nombre, tamaño o recuento mínimo de archivos. Mantiene las 20.000 carpetas coincidentes más grandes y las clasifica por tamaño de contenido; En su lugar, haga clic en Archivos para clasificar por número de archivos. Utilice un patrón como `node_modules` o `*cache*`, o `*` para inspeccionar todo lo que hay debajo de la raíz elegida. Los totales cuentan el contenido del archivo, no se garantiza el espacio en disco recuperable. Las filas de carpetas anidadas se superponen. Los enlaces simbólicos nunca se siguen, los análisis permanecen en el sistema de archivos inicial y una carpeta que se contiene a sí misma (a través de un montaje de enlace o una carpeta vinculada) se ingresa solo una vez; dichos límites y los niños ilegibles o excluidos hacen que los totales estén incompletos y se muestran con ≥. Los análisis de directorios utilizan metadatos y no leen el contenido de los archivos. Los resultados aparecen después de la medición; el escaneo sigue siendo cancelable. **Árbol de tamaño de carpeta** muestra el total raíz y sus subcarpetas en el panel actual. Derecha expande una rama o ingresa a una rama expandida; Lo dobla a la izquierda o selecciona su padre. Enter abre la carpeta real. Haga clic en una flecha de rama para plegar o expandir, o en un encabezado de columna para ordenar a los hermanos por nombre, tamaño o número de archivos. Esc esconde el árbol; Alt-Shift-R restaura sus ramas, marcas y cursor. Las operaciones de archivos utilizan rutas reales, y marcar un padre y un hijo envía solo al padre a un trabajo de copia o eliminación.

Las búsquedas por nombre omiten las carpetas generadas mientras **Omitir resultados de compilación, cachés y archivos ignorados** está marcado: `node_modules`, `.git`, las cachés y entornos virtuales de Python, carpetas como `target` y `dist` cuando hay un archivo de compilación al lado (`Cargo.toml`, `package.json`), y cualquier carpeta marcada con un archivo `CACHEDIR.TAG`. También se omiten los archivos que excluyen `.gitignore`, `.ignore`, `.rgignore` o `.borgignore`, junto con los globs de `[search] exclude`. Una carpeta omitida cuyo propio nombre coincide sigue apareciendo, y la línea de contexto cuenta las carpetas omitidas. `excluded:yes` busca en todo una vez, y `hidden:yes` incluye los archivos ocultos. Los análisis de tamaño siempre leen todo. El historial de búsqueda se queda en esta computadora y solo usted puede leerlo; `history = false` en `[search]` deja de guardarlo.

**Buscar → Indexar esta carpeta para búsqueda instantánea** mantiene un índice de todos los nombres bajo la carpeta activa, de modo que Ctrl-F en ella, o en cualquier carpeta inferior, responde al instante; la línea de contexto indica cuándo se creó el índice. Los índices se reconstruyen en segundo plano después de iniciar Borg y cuando tienen más de 15 minutos, y el índice anterior responde hasta que el nuevo está completo. Una búsqueda que no omite nada, o que usa otras exclusiones, recorre las carpetas. Elija de nuevo el elemento para dejar de indexar y borrar el índice. Los índices se guardan en la carpeta de caché junto con los análisis de tamaño; `[index]` en la configuración enumera las carpetas indexadas.

Para medir tamaños mientras mantiene abiertos los paneles de archivos normales, presione **Alt-I** o elegir **Buscar → Contar tamaños en segundo plano**. Borg escanea cada carpeta debajo del directorio actual, incluye archivos ocultos y completa los totales de las carpetas a medida que finalizan. Puede navegar o utilizar la consola mientras se ejecuta. Las carpetas que aún se están midiendo muestran un total acumulado creciente marcado `≥`y los totales permanecen cuando sales de una carpeta y regresas. El pie de página del panel muestra el progreso; **Alt-I** nuevamente, o un clic en el pie de página, detiene el escaneo y mantiene los totales medidos hasta el momento. Iniciar un trabajo de copia, movimiento o eliminación detiene el escaneo de tamaño en curso. Los escaneos terminados comparten su caché con el árbol de tamaño y el análisis de espacio.

Los escaneos completos se guardan localmente. Tanto el análisis de espacio como el árbol de carpetas reutilizan un análisis guardado para la misma política de raíz y de archivos ocultos. Al reabrir una raíz medida se utiliza esa instantánea inmediatamente: `~` marca los tamaños almacenados en caché, con la fecha de medición encima del árbol. **Ctrl-R** vuelve a escanear en cualquier tamaño de vista; **Ctrl-C** cancela. El árbol anterior permanece visible durante una actualización y sobrevive a la cancelación o falla. Los listados de carpetas normales también muestran los totales almacenados en caché disponibles. Los cambios fuera de Borg pueden dejarlos obsoletos; la navegación no activa escaneos recursivos. Borg copiar, mover y eliminar trabajos invalidan las instantáneas que se superponen a sus rutas modificadas.

El índice local SQLite conserva cada carpeta medida hasta para 16 raíces. Los árboles cargan a los niños a medida que las ramas se expanden; una rama muestra hasta 20.000 carpetas y el análisis de espacio plano mantiene las 20.000 coincidencias más grandes. Estos límites de visualización no detienen el escaneo ni reducen los totales medidos. Una fila "Archivos en esta carpeta" representa los bytes almacenados directamente en una carpeta expandida. Los errores y los subárboles omitidos marcan los totales como incompletos. Los archivos de caché viven en `~/.cache/borg-commander`, respetando `XDG_CACHE_HOME`; `BORG_CACHE_DIR` anula la ubicación. Son medidas desechables, separadas de las colecciones. Los archivos vinculados cuentan una vez por ruta. Estos son tamaños de contenido lógico, no uso de disco físico o recuperable.

Las acciones de Git y Archive actualmente funcionan desde paneles de directorio reales. Utilice Revelar original para acceder a ellos y obtener un archivo recopilado.

## llaves

La barra de acceso directo de dos filas se encuentra debajo de la línea de comando. Las teclas de función están en la primera fila; la búsqueda, las colecciones y otros atajos contextuales están en el segundo. Se puede hacer clic en ambas filas. Los avisos reemplazan temporalmente la segunda fila.

| clave | acción |
| --- | --- |
| Arriba, Abajo, Re Pág, Av Pág, Inicio, Fin | Mover |
| Enter, cierto | Abrir carpeta/ejecutar o ver archivo |
| Izquierda | Ir a la carpeta principal |
| Tab | Cambiar de panel o enfocar la vista previa rápida/volver a la lista de archivos |
| Ctrl-O | Mostrar el caparazón / volver a los paneles |
| Alt-I | Cuente los tamaños de carpeta en segundo plano/deje de contar |
| Ctrl-S | Búsqueda rápida en la carpeta. |
| Ctrl-Q | Vista rápida: el otro panel muestra una vista previa del archivo bajo el cursor (texto, imagen o carpeta) |
| Ctrl-L | Información: el otro panel muestra tamaño, modo, propietario, fechas y espacio libre. |
| Insertar, Ctrl-T, Shift-Up/Abajo | Marcar archivos |
| `+` `-` `*` | Seleccionar, deseleccionar o invertir por patrón (`*.rs`, `build/`) |
| Alt-Enter | Pon los nombres marcados (o el actual) en la línea de comando |
| Ctrl-Insert / Alt-Shift-Insert | Copiar los nombres marcados / sus rutas completas |
| Shift-Enter, Ctrl-J | Inserte una nueva línea en el cursor de la línea de comando; Enter ejecuta el comando, Ctrl-C lo borra |
| Alt-A / Alt-Shift-A | Coloque la ruta de la carpeta de este panel/otro panel en la línea de comando |
| Shift-PgUp / Shift-PgDn | Desplazar el historial de salida de la consola |
| Shift-Home / Shift-End | Salida de consola más antigua/volver al aviso en vivo |
| Ctrl-P / Ctrl-N, Ctrl-R | Historial de Shell: comando anterior/siguiente, búsqueda (manejado por su Shell) |
| Alt-. | Mostrar archivos ocultos |
| Alt-Shift-Left/Derecha | Cambiar el tamaño de los paneles |
| Ctrl-U | Intercambiar paneles |
| Alt-J | Trabajos: pausar o cancelar copias en ejecución, movimientos y eliminaciones |
| Alt-M | Monitor de actividad: procesos y memoria. |
| Alt-C | Docker: componer proyectos y contenedores |
| F1 | Acerca de |
| F2 | menú archivo |
| F3 | Ver (texto o imagen) |
| F4 | Editar con `$VISUAL` / `$EDITOR` |
| F5 / F6 | Copiar/mover al otro panel |
| F7 | hacer carpeta |
| F8 | Eliminar |
| F9 | barra de menú |
| F10 | Salir |

Tab, Enter, Inicio, Fin, Izquierda y Derecha actúan en los paneles mientras la línea de comando está vacía. Una vez que escribes un comando, esas claves van al shell. Ctrl-P, Ctrl-N y Ctrl-R usan el historial de comandos de su shell. Shift-Enter (o Ctrl-J) inserta una nueva línea sin ejecutar el comando. Los comandos multilínea escritos y recuperados expanden la misma área de aviso, reduciendo los paneles y manteniendo el cursor visible. El shell conserva sus claves habituales de edición y finalización.

Las copias llegan al portapapeles de su computadora a través del terminal, también por SSH y en un espejo que usted controla. Borg también guarda sus últimas 20 copias: Ctrl-V o Shift-Insert pega la más reciente en los campos propios de Borg, como los diálogos y la búsqueda en memoria, incluso donde el terminal no llega a su portapapeles. En la línea de comandos Ctrl-V sigue siendo del shell, así que pegue allí con la tecla de pegar de su terminal. El menú Archivo ofrece ambas copias para teclados sin Insert; las copias demasiado grandes para el terminal se quedan en Borg.

Si su terminal reserva teclas F, presione **Esc, luego un dígito** en 1,5 segundos: Esc 1–9 son F1–F9; Esc 0 es F10. Por ejemplo, Esc 9 abre los menús y Esc 5 copia. Mientras Borg espera el dígito, la fila de teclas de función muestra los dígitos que se deben presionar. Esc primero cierra lo que está abierto (un aviso, resultados, la vista rápida o Información); un segundo Esc se cancela. Alt-1–0 también funciona cuando Alt llega a la terminal. El macOS, habilite la Opción como Meta en su terminal si es necesario.

## Ratón y consola

Haga clic en una fila para enfocarla, haga doble clic para abrirla y haga clic derecho para marcarla. Haga clic en la barra de menú o en las acciones inferiores para usarlas. La rueda se mueve a través de las entradas del panel.

Ctrl-O cambia entre paneles y consola. El shell sigue la carpeta del panel activo antes de escribir o abrir la consola. En zsh cambia de carpeta silenciosamente y vuelve a dibujar el mensaje; en bash y fish un corto `cd` El comando aparece en la consola en su lugar. Los comandos y Ctrl-C mantienen visible una consola abierta explícitamente. “Regresar a los paneles después del comando” se aplica a los comandos iniciados desde los paneles. En el símbolo del shell, Opción/Alt-Left y Opción/Alt-Right se mueven por palabras.

En la consola, la rueda desplaza el historial de salida sin cambiar el comando que se escribe. Shift-PgUp/AvPág desplazarse por una página; Shift-Home va a la salida más antigua; Shift-End regresa al mensaje en vivo. Al escribir, también se regresa al mensaje en vivo. Utilice el gesto de selección de su terminal, generalmente Shift-drag, luego su acceso directo de copia normal para copiar la salida visible.

## complemento de git

Git es un complemento incluido, habilitado de forma predeterminada. F9 → Opciones → Complementos abre su casilla de verificación; desactivarlo oculta el menú de Git y las columnas de estado y detiene los análisis de estado en segundo plano. Requiere Git en tu PATH.

Dentro de un repositorio, los paneles muestran la rama y un estado de dos caracteres al lado de cada archivo. El carácter de la izquierda describe el índice (cambios por etapas); a la derecha se describe el árbol de trabajo. `M` significa modificado, `A` añadido, `D` eliminado, `R` renombrado, `??` sin seguimiento, y `U` un conflicto. Las carpetas resumen los cambios debajo de ellas. Los archivos limpios e ignorados no tienen marcador. Fuera de los repositorios, los nombres de archivos utilizan el espacio adicional.

El menú Git ofrece estado del repositorio, diferencias preparadas y no preparadas, confirmaciones recientes y preparación/desorganización para archivos marcados (o el archivo actual cuando no hay nada marcado). Las diferencias y el historial se abren en el visor normal. Unstage conserva los archivos de trabajo. El estado se actualiza después de la navegación, los comandos y las acciones de Git, y cada cinco segundos mientras los paneles están inactivos. El estado de actualización en el menú Git se vuelve a escanear inmediatamente.

## Docker complemento

**Alt-C** o **Comando → Docker** enumera sus contenedores en el panel activo, agrupados por proyecto de Compose, con los contenedores fuera de Compose al final. El otro panel muestra el contenedor seleccionado: su estado, puertos, redes, montajes, comando y política de reinicio, con CPU en vivo, uso de memoria, red y disco y un gráfico del último minuto. En un proyecto muestra la carpeta del proyecto, sus contenedores y su uso combinado. Esc cierra la vista.

| clave | acción |
| --- | --- |
| ↑ ↓, Re Pág, Av Pág, Inicio, Fin | Mover |
| ← → / Enter | Doblar o desplegar un proyecto; Enter en un contenedor centra sus detalles |
| S/X/R | Iniciar/detener/reiniciar el contenedor, o los contenedores del proyecto |
| pag | Pausar o reanudar |
| f | Explorar los archivos del contenedor |
| E, Alt-2 | Abrir un caparazón en el contenedor o regresar a su caparazón abierto |
| Alt-1 | Mostrar de nuevo la visión general del contenedor, también desde el interior de su caparazón. |
| `/` | Filtrar por nombre, servicio, imagen o proyecto |
| Tab | Cambiar entre la lista y los detalles |
| Alt-W | Maximizar el panel enfocado |

Para detener y reiniciar, pregunte primero. En un proyecto, una acción solo toca los contenedores que cambia, por lo que Iniciar inicia los detenidos.

**F** en un contenedor en ejecución cierra la vista Docker y muestra el sistema de archivos del contenedor en ese panel, comenzando en su directorio de trabajo. El título del panel nombra el contenedor. El otro panel mantiene su carpeta de host, por lo que **F5** copias en cualquier sentido, y **F3** vistas, **F7** hace una carpeta y **F8** elimina dentro del contenedor. Las búsquedas, los tamaños, las vistas previas y la edición permanecen en el host. **Alt-C** vuelve a la vista Docker; `..` en la raíz del contenedor devuelve el panel a la carpeta host que mostraba antes. Necesidades de navegación `sh` en el contenedor, por lo que no se pueden enumerar imágenes mínimas sin shell.

Bajo los archivos de un contenedor, la línea de comando sigue al panel: se convierte en el shell de ese contenedor, iniciado en la carpeta del panel, con el nombre del contenedor al inicio de su mensaje. Los comandos se ejecutan dentro del contenedor, el panel se actualiza cuando finalizan y aparece un `cd` allí se mueve el panel. Ctrl-O muestra la consola de ese contenedor, marcada **▣ consola de nombres**; en un panel de host, la línea de comando y Ctrl-O son nuevamente del host, y se conserva todo lo que haya escrito allí. El shell del contenedor es el mismo que su pestaña Shell en la vista Docker.

Apagar **Docker — la línea de comando sigue a los paneles del contenedor** en Opciones → Complementos para mantener la línea de comando en el host. Debajo de los archivos de un contenedor, se atenúa y se marca **⌂ anfitrión**y Enter no ejecuta nada hasta que un panel host esté activo, por lo que un comando nunca se ejecuta en la máquina equivocada.

**E** abre un shell en el contenedor (bash cuando la imagen lo tiene, de lo contrario `sh`) en la pestaña Shell del panel de detalles y le envía sus claves, incluidas las teclas de función. **Alt-1** muestra la descripción general y **Alt-C** vuelve a la lista; la rueda del ratón desplaza su historial. Un shell sigue ejecutándose mientras miras otros contenedores, cierras la vista Docker o usas la consola del host, y su contenedor está marcado `›_` en la lista y contó con el atajo Alt-C. Se cierra cuando sales, cuando su contenedor se detiene o cuando Borg sale, que pregunta primero mientras los shells están abiertos. El complemento utiliza el `docker` CLI y su contexto actual, por lo que funciona con Docker Desktop, OrbStack, Colima y motores remotos por igual. Se puede desactivar en Opciones → Complementos.

## Claude y otros agentes (MCP)

`borg mcp` permite que un agente de IA como Claude trabaje en el borg que tiene abierto, en su máquina o en un servidor al que accede a través de SSH. No es necesario instalar nada además de borg: el agente se inicia `borg mcp` sí mismo, a través de SSH para un servidor, y se conecta a la sesión borg abierta allí. El servidor no necesita ningún puerto abierto ni acceso a Internet.

Con espacios de trabajo con nombre, `borg sessions list` muestra sus identificaciones y nombres; `borg sessions create NAME` añade uno y `borg --session NAME` lo abre. `borg mcp` puede llamar `list_sessions`,
`list_remote_sessions`, y `use_session` para elegir un objetivo para esa conexión MCP. Una llamada a una herramienta también puede proporcionar una `session` ID para anulación de una sola llamada. El objetivo seleccionado permanece privado para esa conexión MCP.

`borg service install`, `enable`, y `start` configurar el servicio en segundo plano opcional. Es propietario de la conexión de emparejamiento de la máquina y mantiene en ejecución las sesiones con nombre cuando sus terminales se desconectan. `borg service attach NAME` se vuelve a conectar a uno. Ctrl-] se desprende sin terminar su caparazón. uso `borg service stop`, `disable`, o `uninstall` para controlar el ciclo de vida del servicio. En un contenedor sin systemd, haga que se ejecute el supervisor de procesos del contenedor `borg service run`; el `install` y `enable` Los comandos requieren un host systemd en Linux.

Después del emparejamiento, `borg --remote MACHINE:SESSION` abre el sistema de archivos de esa sesión en el panel local izquierdo. Enter o F3 obtiene una vista previa de un archivo remoto a través de una descarga local temporal. **F5** copia archivos y carpetas marcados entre paneles locales y remotos, o entre dos hosts emparejados. El cuadro de diálogo de destino identifica al cliente o host. Las copias se ejecutan como trabajos en segundo plano con bytes de progreso, pausar/cancelar y sobrescribir, omitir o mantener ambas opciones. Las carpetas incluyen carpetas vacías y enlaces simbólicos; Los permisos ejecutables y los tiempos de modificación se conservan para los archivos normales. Un archivo se vuelve visible en su destino final sólo después de que su contenido llega por completo. F6 mover y F8 eliminar no están disponibles en los paneles remotos. El panel derecho permanece en su computadora. con **Opciones → Complementos → Seguir** habilitado, la línea de comando y **Ctrl-O** utilice un shell interactivo en el host remoto. Su terminal se renderiza localmente; Las teclas, pegar, Ctrl-C y los programas de terminal se ejecutan de forma remota. Cada sesión remota conserva su propio shell y la línea de comandos sin terminar mientras cambia de panel. Al seleccionar un panel de cliente se restaura el shell local. Aplicaciones que usan la pantalla alternativa y aplicaciones en primer plano que usan entrada sin formato, como `top`, recibe todas las claves mientras se ejecuta, incluido Ctrl-O. El cambio de tamaño del terminal llega al PTY remoto. Salga de la aplicación usando sus propias teclas (`q` en `top`); Borg regresa a los paneles si lo inició desde la línea de comando del panel, o deja la consola remota abierta si la inició allí. Los propios paneles y carcasa del control remoto Borg son independientes de esta consola. uso `borg get MACHINE/SESSION:PATH [TO]` y `borg put FROM MACHINE/SESSION:PATH` para transferencias de archivos. En local Borg, presione **Alt-R**, usa la parte inferior **Hosts remotos** acceso directo o elija **F9 → Remoto → Examinar hosts y sesiones…** para ver las máquinas emparejadas y sus sesiones nombradas. Enter abre los archivos de la sesión seleccionada en el panel que abrió el selector, reemplazando la ubicación anterior de ese panel. Presione **M** en el selector para reflejar su pantalla; **R** actualiza la lista. **Tab** cambia entre **Anfitriones** (acceso saliente) y **Clientes** (acceso entrante). Los clientes enumeran las computadoras emparejadas con este Borg, sus huellas digitales de clave pública, si están conectadas y su conexión cuenta. Una computadora aparece una vez incluso cuando un espejo usa varias conexiones. El estado se actualiza mientras el selector está abierto, incluso dentro de una sesión cuyas conexiones pertenecen al servicio en segundo plano. El emparejamiento es direccional: un cliente permitido no se convierte automáticamente en un host cuyos archivos este Borg pueda explorar. Cuando el propietario de la conexión no puede informar de su estado, los clientes emparejados siguen visibles con **Estado de conexión no disponible**. Mientras se conecta un espejo, el selector muestra un estado animado y **Esc** cancela inmediatamente. Cada paso de la conexión tiene un plazo de 10 segundos, con un límite de 30 segundos para todo el intento. Un intento fallido mantiene abierto el selector e indica el paso que falló; **M** reintenta la sesión seleccionada. En el recolector, **F2** cambia el nombre de la etiqueta del host seleccionado para cada sesión en ese host; **Shift-F2** cambia el nombre de la etiqueta de sesión seleccionada. Estas etiquetas se guardan en este cliente y aparecen en el selector, los títulos del panel y el pie de página de la consola. Deje un nombre en blanco para restaurar el original. Los detalles conservan los nombres originales del host y de la sesión. Las etiquetas están vinculadas a la clave del host emparejado y al ID de sesión; Los destinos y nombres de los comandos en el host remoto permanecen iguales.

**F8** sobre un host no disponible olvida su emparejamiento en este cliente. **Shift-F8** olvida el host seleccionado aunque esté en línea. La confirmación tiene **No** como opción predeterminada. Olvidar un host cierra los paneles y las consolas de este cliente para ese host; las sesiones y los archivos del host se conservan. La identidad del cliente y sus demás emparejamientos se mantienen. Vuelva a emparejar para reconectar.

**F8** o **Eliminar** elimina la sesión del host seleccionado después de una confirmación cuyo valor predeterminado es **No**. La eliminación cierra esa sesión y sus consolas remotas y elimina sus paneles Borg guardados y su historial de terminal; Se conservan los archivos y carpetas del host. La sesión predeterminada está protegida. Una sesión independiente que actualmente alberga la conexión no puede eliminarse por sí sola; ciérrelo en el host y ejecute Borg en otra sesión primero. La eliminación fallida mantiene la sesión visible para volver a intentarlo. Las sesiones administradas por el servicio se detienen antes de que se elimine su estado guardado y el servicio no las vuelve a crear.

El selector distingue los hosts salientes de los clientes entrantes; el mismo Borg puede cumplir ambos roles. El menú Remoto también ofrece **Anfitrión montado en espejo** y **Comparte este Borg como anfitrión…**. El selector muestra la versión Borg de cada host; Al seleccionar una sesión se muestra el sistema operativo/arquitectura, el servicio en segundo plano o el modo de terminal, la latencia de solicitud y la dirección de retransmisión. Los hosts se verifican simultáneamente, por lo que un host no disponible no oculta los resultados a los demás. Un host que no puede informar de su versión muestra **versión no disponible**; la versión del relé no es la versión del host. Mantenga Borg actualizado en ambas computadoras: un host sin una función remota muestra un mensaje de actualización.

Las carpetas remotas llegan en fragmentos con un indicador de carga y un recuento de entradas recibidas; la navegación cancela la solicitud. `borg mirror MACHINE:SESSION` muestra la pantalla en vivo de la sesión remota en su terminal local; Alt+Shift+M abre el mismo espejo desde un Borg local con un panel remoto. **Ctrl-G** sale del espejo y regresa a los paneles locales: mantenga presionada la tecla Control y presione G. También puede hacer clic en la línea inferior del banner. El espejo mantiene visible su estado de control en la línea de arriba; Ctrl-] también sale. `--take` toma explícitamente el control de otro espectador. Un habitual `borg` o `borg --session NAME` se puede reflejar mientras se está ejecutando; no se requiere ningún servicio en segundo plano. Su terminal local permanece visible y la entrada del teclado o mouse local recupera el control del espejo. El espejo sigue el diseño del terminal host y se ajusta para adaptarse a una ventana de cliente más pequeña; ampliar al cliente revela el resto. El host en segundo plano opcional mantiene vivas las sesiones después de que se cierra su terminal original y permite al cliente controlador cambiar el tamaño de su PTY. Si otro terminal controla la sesión remota, el espejo se abre en modo de sólo visualización; sus claves no cambian la sesión remota. Presione **Ctrl-T** o haga clic en la línea de estado para tomar el control, desconectar el otro terminal o utilizar `borg mirror MACHINE:SESSION --take` para tomar el control al iniciarlo. Después de tomar el control, la primera tecla activa una pantalla remota inactiva; Las siguientes teclas y entradas del ratón llegan a él. Un espectador que se queda atrás recibe la pantalla actual en lugar de cuadros de animación antiguos. Escribir `exit` en el shell principal de Borg (Bash, Zsh o Fish) pide confirmación. **No** está seleccionado de forma predeterminada y mantiene en ejecución el mismo shell, variables, directorio y espejos.

Para conectarse, presione F1 y luego C (o F9 → Remoto → Compartir esto Borg como anfitrión…). Borg muestra dos comandos para pegar en una terminal de su computadora: uno inicia Claude Code ya conectado a este borg, el otro mantiene la conexión en Claude Code para cada sesión. Tab elige uno, Enter lo copia al portapapeles de su computadora a través del terminal, que funciona a través de SSH en iTerm2, Ghostty, kitty, WezTerm y Alacritty (en tmux, con `set -g set-clipboard on`). Donde el terminal no lo acepta, como en muchos terminales de navegador, F3 muestra el comando solo como texto plano, con el ratón libre para seleccionarlo; cualquier tecla vuelve a borg. A través de SSH, borg completa la dirección a la que ingresaste; Si su computadora llega al servidor con otro nombre, como un alias SSH o mediante un host de salto, escriba ese nombre en su lugar. SSH debe iniciar sesión sin que se le solicite una contraseña: una clave o un agente SSH. `borg mcp setup prod` imprime lo mismo para Claude Code y Claude Desktop desde la línea de comando.

Conectado, el agente ve lo que usted ve: la pantalla misma como texto, ya sea que esté en los paneles o en la consola, ambos paneles, los archivos resaltados y marcados, dónde se ejecuta la línea de comando y lo que se escribe allí. Nombra lugares como ubicaciones: un camino absoluto o `~/…` en la máquina borg se ejecuta, `docker://web/etc/nginx` dentro de un contenedor, o una ruta relativa al panel activo. Sin preguntar, lee carpetas, archivos (por rangos de líneas o líneas que contienen algo de texto), los procesos más ocupados y los contenedores, y muestra un archivo o carpeta en el otro panel.

Cualquier cosa que cambie la máquina lo espera en un cuadro de diálogo que lo muestra todo: un comando y dónde se ejecuta, un archivo nuevo o una edición como diferencia. Los largos se desplazan con las teclas de flecha, PgUp y PgDn o la rueda del mouse, y F3 muestra el diálogo en pantalla completa. Se selecciona No hasta que elija Sí; se rechaza una edición si el archivo cambió mientras usted estaba decidiendo. Para una computadora emparejada, `approval_mode` pregunta una vez antes de habilitar Aprobar todo para esa sesión. Su insignia permanece visible; haga clic en la insignia o presione Ctrl+Alt+A para desactivarla inmediatamente. También se apaga cuando se cierra esa conexión emparejada. Las aprobaciones de emparejamiento permanecen separadas. Los comandos se escriben en un shell que usted observa y se mantienen fuera de su historial: los comandos de esta máquina en la propia consola de borg, que se muestra mientras se ejecutan, para que pueda responder a un mensaje como el de sudo; comandos en un contenedor en el shell de ese contenedor en la vista Docker. El shell mantiene su carpeta y variables entre comandos.

El agente también puede escribir texto para que usted lo revise: un comando cuando se le solicite o un script en vim o nano abierto en la consola. No se ejecuta nada: no se presiona Enter y el texto con varias líneas ingresa como un pegado, que los shells y editores insertan sin ejecutarse.

### Borg nube

Borg la nube permite que Claude llegue a un borg al que no tiene ruta SSH: un servidor que abres en una terminal de navegador, un contenedor, una máquina detrás de NAT. Ambos lados se conectan a un relé y el relé los une.

1. En borg en el servidor, presione F1 y luego C. La primera opción es `borg pair` con un enlace; Enter lo copia. El enlace funciona una vez, durante diez minutos. Debajo está el relé: pegue la dirección de otro relé allí y presione Enter para guardarlo y obtener un enlace a través de él.
2. Ejecútelo en una terminal de su computadora, donde también está instalado borg. Imprime un código de emparejamiento: seis emoji, cada uno con su nombre, como `🐱 Cat · 🦁 Lion · 🐎 Horse`.
3. Borg en el servidor pregunta si desea emparejar, mostrando el nombre de la computadora y el código. Elija Sí si los códigos coinciden. `borg pair` luego ofrece agregar el servidor a Claude Code, como    `claude mcp add borg-NAME -- borg mcp remote NAME`.

El código proviene del apretón de manos encriptado que los dos borgs acaban de terminar, por lo que es el mismo en ambas pantallas solo cuando su computadora es la que habla con el servidor. Alguien que consiguiera el enlace y lo utilizara primero mostraría al servidor un código diferente y no podría elegir uno que coincida con el suyo. Cuando una terminal dibuja emojis más estrechos que borg los presenta, borg muestra solo los nombres; comparar los nombres.

A partir de entonces, Claude llega a ese borg siempre que esté abierto allí. `borg pair` solo enumera los servidores emparejados, y `borg pair --forget NAME` olvida uno; en el servidor, F8 en el cuadro de diálogo Acceso remoto olvida todas las computadoras emparejadas. A través de borg nube Claude también llega `download` y `upload`, que copian archivos entre el servidor y su computadora; `borg get NAME:PATH [TO]` y `borg put FROM NAME:PATH` haz lo mismo desde tu terminal. Las cargas esperan su aprobación en borg. Cada aprobación nombra la computadora que solicitó.

La conexión está cifrada de extremo a extremo. Cada borg guarda una clave privada, que solo usted puede leer, en su carpeta de estado. El emparejamiento intercambia las claves públicas y cada conexión se abre con un protocolo de enlace de Noise IK vinculado a ellas, por lo que el relé solo reenvía texto cifrado y no puede responder en nombre de ninguno de los borg. Las llamadas desde otra computadora pueden leerse sin preguntar, como las locales; Opciones → Complementos → "preguntar antes de que otra computadora lea" hace que esas lecturas también esperen su aprobación. Borg no mantiene conexión con el relé a menos que esté emparejado o muestre un enlace de emparejamiento, y ninguna cuando la nube borg está apagada en Opciones → Complementos.

Borg guarda el relé en la configuración, como `relay = "wss://relay.example.com"`. Una computadora llega al servidor a través del relé con el que se emparejó; Después de que el servidor cambie el relé, empareje la computadora nuevamente. Los Borg en una máquina comparten una clave, por lo que solo el primero que se abre se conecta al relé; los demás dicen cuál tiene borg nube.

Mientras un agente está conectado, **◈ Remoto** se encuentra al comienzo de la fila de atajos; haga clic en él para ver el registro de lo que hizo: lo que miró, leyó, mostró y escribió, y cada comando y cambio, aprobado o rechazado. Borg mantiene el registro solo en la memoria hasta que sale; Para no guardar ningún registro, desactívelo en Opciones → Complementos.

### Ejecutando un relevo

`borg-relay` es el relevo por el que se encuentran los borgs. En un servidor Linux cuyo dominio apunta a él, con el puerto 443 libre, un comando lo instala como un servicio con un certificado TLS de Let's Encrypt:

```sh
curl -fsSL https://raw.githubusercontent.com/0xcd21h/borg-commander/main/scripts/install-relay.sh | sudo sh -s -- relay.example.com you@example.com
```

Ejecútelo nuevamente para actualizar.

En Dokploy, Coolify u otro host Docker cuyo proxy agregue TLS, ejecute la imagen `ghcr.io/0xcd21h/borg-relay:latest` y darle un dominio en el puerto 8080: en Dokploy, una aplicación de esa imagen Docker con un dominio en el puerto del contenedor 8080 y HTTPS activado; en Coolify, un recurso de esa imagen Docker con el puerto 8080 expuesto y un dominio. O también toma [embalaje/relé/docker-compose.yml](../../../packaging/relay/docker-compose.yml) como servicio de redacción. La imagen corre `borg-relay --behind-proxy`: el proxy sirve HTTPS y pasa WebSockets, y el relé cuenta sus límites según la dirección del cliente en la que informa el proxy `X-Forwarded-For`. Se ejecuta como un usuario sin privilegios y tiene una verificación de estado (`GET /health`).

En un host Docker sin dicho proxy, la misma imagen sirve a TLS:

```sh
docker run -d --restart unless-stopped -p 443:443 -v borg-relay:/data ghcr.io/0xcd21h/borg-relay --domain relay.example.com --cache /data
```

Detrás de otro proxy que agrega TLS, ejecute `borg-relay --behind-proxy --listen 127.0.0.1:8080`; Úselo solo detrás de un proxy, ya que el relé confía en la dirección del cliente que informa el proxy. El relé mantiene límites por dirección, en servidores y sesiones, en tamaño de trama, ancho de banda, bytes por sesión, tiempo de inactividad y duración de la sesión. No almacena nada más que su certificado.

Los registros de retransmisión solo cuentan: una vez por minuto, cuando cambiaban, una línea como

```text
stats: connections=3 servers=2 sessions=1 joined=14 carried=3.2MiB limited=0
```

con las conexiones que tiene, los servidores registrados y las sesiones abiertas, y desde que comenzó, las sesiones unidas, los bytes que llevaban y las conexiones y sesiones con un límite rechazado o finalizado. Para depurar, `--log`, o la variable de entorno `BORG_RELAY_LOG=1` para la imagen, también registra el registro de cada servidor por el inicio de su clave, cada sesión con los bytes que transportaba y cada límite alcanzado con la dirección del cliente.

## Buscar, trabajos y espectadores

Ctrl-S abre la búsqueda rápida. Escriba un nombre o un globo como `*.rs`; Abajo y Arriba se mueven entre coincidencias, Insertar marca una coincidencia, Ctrl-A marca todas las coincidencias y Enter o Esc cierra la búsqueda. La coincidencia no distingue entre mayúsculas y minúsculas a menos que la consulta contenga letras mayúsculas.

F5, F6 y F8 inician copiar, mover y eliminar trabajos. Alt-J abre la lista de trabajos para pausar o cancelar.

El cuadro de diálogo Copiar tiene una opción opcional. **Optimizar la copia para discos duros** casilla de verificación. Utilice Tab para enfocarlo y la barra espaciadora para alternar, o haga clic en él. Comienza sin marcar para cada copia. Este modo lee archivos pequeños en la memoria en lotes y luego escribe cada lote, lo que puede reducir la búsqueda de origen/destino al copiar dentro de un disco duro. Los lotes contienen hasta 64 MiB de datos de archivos o 4096 archivos; Los archivos de más de 1 MiB se transmiten normalmente. El rendimiento depende de la unidad, el sistema de archivos y el almacenamiento en caché del sistema operativo; puede ser más lento entre unidades separadas. Para conflictos de nombres: `o` sobrescribe, `s` salta, `k` mantiene ambos, `a` sobrescribe todo, `n` omite todo y Esc cancela. Las copias se escriben en archivos temporales y se les cambia el nombre una vez finalizadas.

F3 abre el visor. En archivos de texto, flechas y desplazamiento PgUp/PgDn, `w` cambia de envoltura, y Esc o `q` cierra. En imágenes, Izquierda/Derecha o `p`/`n` seleccione la imagen anterior o siguiente. Se utilizan protocolos de imagen cuando están disponibles; otros terminales muestran bloques de colores.

Ctrl-Q muestra una vista previa rápida en el otro panel. Tab lo enfoca; Las flechas, Re Pág/Av Pág, Inicio/Fin y la rueda del ratón desplazan el documento. Tab o Esc regresa a la lista de archivos. Ctrl-L muestra información del archivo. F3 expande una vista previa de texto en el visor; Al cerrarlo se regresa a la vista previa con su posición, ajuste y modo de fuente conservados. Ambos usan el mismo lector de archivos delimitados, por lo que al abrir un archivo de texto grande se lee cerca de la primera página en lugar de cargar el archivo completo.

Ambos visores de texto destacan lenguajes de programación, marcas y archivos de configuración comunes, incluidos Rust, Python, JavaScript, TypeScript/TSX, Go, C/C++, Java, C#, Ruby, PHP, Lua, SQL, HTML/CSS, JSON, YAML, TOML, scripts de shell y Dockerfiles. Markdown se abre formateado de forma predeterminada. Presione `s` o F4 en cualquiera de los visores para alternar la vista de origen. Se admiten encabezados, énfasis, listas, tareas, enlaces, tablas y bloques de código delimitados. Los archivos Markdown de más de 2 MiB se abren como fuente para mantener la memoria limitada. F4 abre el archivo seleccionado con `$VISUAL` o `$EDITOR`.

## Configuración

Cambia el idioma de la interfaz en **F9 → Opciones → Idioma**. Elige inglés, chino simplificado, portugués de Portugal, español o ruso y pulsa Enter para guardar. Esc cancela. La elección se guarda en `config.toml` como `language = "en"`, `"zh-CN"`, `"pt"`, `"es"` o `"ru"`. `BORG_LANGUAGE` tiene prioridad sobre la configuración guardada. Si no se indica ninguno, Borg consulta `LC_ALL`, `LC_MESSAGES` y `LANG`, usando inglés para idiomas no compatibles. Los comandos, rutas, nombres de archivos y atajos no se traducen. El menú de idiomas del sitio controla el idioma de la guía de forma independiente.

Los ajustes se guardan en `~/.config/borg-commander/config.toml`, o el directorio seleccionado por `$XDG_CONFIG_HOME` o `$BORG_CONFIG_DIR`. F9 → Opciones → Configuración abre casillas de verificación; Un espacio o un clic alternan una configuración y la guardan inmediatamente. Esc cierra la ventana.

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

En una terminal local con color verdadero, Borg se abre con una breve introducción de su logotipo; cualquier tecla o clic lo omite. Sobre SSH y en terminales sin color verdadero, comienza directamente en los paneles.

Las carpetas del panel, la posición del cursor, la división, el modo de listado y la clasificación se guardan en `~/.local/state/borg-commander`, o la ubicación seleccionada por `$XDG_STATE_HOME` o `$BORG_STATE_DIR`.

Mientras se ejecuta, Borg mantiene los scripts y archivos de integración de shell copiados de los contenedores en `$XDG_RUNTIME_DIR/borg-commander`, o `borg-commander-<uid>` en la carpeta temporal. Sólo tú puedes abrirlo; uno que alguien más creó, o en el que otros pueden ingresar, se pasa por alto y se le da un nombre nuevo. su `TTL` El archivo comienza con la hora hasta la cual está en uso: cada ejecución de borg adelanta esa hora un día cada hora, por lo que una vez que ha pasado, la carpeta se puede eliminar.

Se incluyen temas personalizados `~/.config/borg-commander/themes/`. Un tema puede ampliar un tema integrado:

```toml
name = "Deep Space"
extends = "neon"

[styles]
cursor = { fg = "#0b0b16", bg = "#00e5ff", bold = true }
dir = { fg = "#b388ff", bold = true }
```

Para crear, probar o contribuir, consulte [Desarrollo](../../../DEVELOPMENT.md).

## Una pequeña distracción

Abra Acerca de con F1 y escriba `2048` jugar en un modal. Las teclas de flecha se mueven, U deshace el último movimiento, R comienza un nuevo juego y Esc lo cierra. Llega a 2048, luego presiona Enter para seguir jugando. El tablero y la mejor puntuación se guardan entre sesiones.


## Actividad y memoria

**Alt-M** o **Comando → Monitor de actividad** abre el monitor de proceso. El complemento del sistema incluido está habilitado de forma predeterminada y se puede deshabilitar en Opciones → Complementos. La lista de procesos ocupa el panel activo, con la descripción general de la memoria del proceso seleccionado enfrente. **Enter** o un doble clic abre ese proceso en una ventana sobre el monitor: su mapa de memoria en un lado y una vista previa de los bytes de la región seleccionada en el otro. **Esc** lo cierra. **Tab** cambia el enfoque. **Alt-W** o el **[+]** El botón maximiza el panel enfocado; **[=]** restaura la división, y **[-]** oculta el monitor. El muestreo se ejecuta una vez por segundo mientras el monitor está abierto; al cerrarlo se detiene el muestreo. Las inspecciones de memoria retienen temporalmente el muestreo hasta que llegue el resultado.

La lista muestra CPU, memoria residente, crecimiento de la memoria y tasas de lectura/escritura del disco. CPU 100% significa un núcleo ocupado. Las páginas residentes compartidas pueden aparecer en más de un proceso, por lo que los valores del proceso RAM no deben agregarse como un total único del sistema. La primera muestra CPU y de tasa de E/S no está disponible hasta que llegue una segunda observación.

| clave | acción |
| --- | --- |
| Enter | Abra el mapa de memoria del proceso/enfoque la vista previa de la región |
| `/` | Filtrar por nombre o PID; ↑↓ todavía se mueve, Enter mantiene el filtro, Esc lo borra |
| C/M/G/N/P | Ordenar por CPU / RAM / crecimiento / nombre / PID; repetir para revertir |
| E/S | Ordenar por tasa de lectura/escritura |
| t | Alternar árbol de procesos |
| espacio | Pausar o reanudar el muestreo |
| R | Actualizar la vista actual |
| Esc | Atrás; cerrar de la lista de procesos |

Haga clic en los encabezados del proceso para ordenar; Haga doble clic en un proceso para abrir su mapa de memoria. La descripción general muestra la memoria residente actual, la memoria máxima observada, el cambio desde que comenzó la supervisión, CPU, el espacio de direcciones virtuales y un gráfico de los últimos 60 segundos de la memoria residente. Los totales de huella, intercambiados, privados y compartidos aparecen una vez leído el mapa de memoria y la plataforma los reporta. **Enter** en el panel de memoria abre su mapa de memoria. El mapa de memoria es una medida realizada bajo pedido. Actualícelo con R después de que el proceso cambie sus asignaciones.

El mapa tiene tres columnas:

- **Huella** es la memoria cargada solo para este proceso: páginas que ha escrito más páginas intercambiadas o comprimidas. Es la cifra a tener en cuenta para detectar fugas y en macOS coincide con la del Monitor de actividad. *Memoria* columna.
- **residente** Son páginas en RAM ahora, incluidas las páginas compartidas con otros procesos.
- **mapeado** es el espacio de direcciones, incluidas las reservas. No es RAM.

Un proceso puede asignar gigabytes, en su mayoría bibliotecas del sistema compartidas con todos los demás procesos, mientras que su huella es de unos pocos megabytes. Las regiones comienzan agrupadas por propósito: montón y asignaciones, pilas de subprocesos, código y datos propios del programa, bibliotecas compartidas, archivos mapeados, memoria compartida, sistema y otras asignaciones, y rangos y guardias reservados. Los grupos y sus miembros se ordenan por espacio, siendo las reservas las últimas. Las bibliotecas y los archivos mapeados enumeran una fila por archivo; expanda un archivo para ver sus regiones. Los nombres de los grupos se deducen de las etiquetas del sistema operativo. Seleccionar un grupo o archivo lo explica y muestra su participación en la huella y sus miembros más grandes. Enter o Derecha expande un grupo o archivo; La izquierda lo contrae, o el que está alrededor de la fila seleccionada. G cambia entre listas agrupadas y sin procesar.

**1 memoria** y **2 abrir archivos** cambie las pestañas de inspección o haga clic en sus etiquetas. La lista de archivos abiertos incluye descriptor, modo de acceso, tipo y ruta o punto final; el otro panel muestra todos los detalles, el tamaño del archivo y el desplazamiento actual cuando esté disponible. En macOS también incluye entradas como `cwd` (directorio actual) y `txt` (ejecutable mapeado). R actualiza la lista. El cambio de pestañas conserva las posiciones de la lista. Las restricciones de acceso ofrecen al administrador explícito existente un reintento. El contenido del archivo no se lee a través de los descriptores de proceso.

Al seleccionar una región legible se capturan automáticamente hasta 64 KiB de ella en el panel opuesto. Si el inicio de la región solo contiene ceros, la captura sigue leyendo hasta la primera página con datos, y la línea sobre los bytes indica en qué parte de la región está la ventana. Los movimientos rápidos se agrupan con un breve retraso; al seleccionar otra región se borra la vista previa anterior y se cancela su lectura pendiente. Las regiones ilegibles muestran un mensaje sin intentar leer. Presione **Tab** o **Enter** para desplazarse por la vista previa. **Esc** allí enfoca la lista; **Esc** desde el mapa vuelve a la lista de procesos con su cursor y su filtro conservados.

Las series de filas de ceros se pliegan en una sola fila, como «· 60K bytes en cero ·». **]** y **[** saltan al bloque de datos siguiente y anterior, y **>** y **<** capturan los 64 KiB siguientes y anteriores de la región. **H** cambia la presentación hexadecimal/texto, **R** vuelve a capturar la ventana mostrada y **D** resalta las diferencias con la captura anterior del mismo proceso y dirección. La presentación de texto muestra ASCII imprimible y sustituye puntos por otros bytes. Las instantáneas son observaciones en vivo; el proceso no se pausa, por lo que una captura no es una imagen atómica de su estado completo. Permanecen en la memoria a menos que se exporten explícitamente. **E** exporta bytes sin formato al directorio del panel de archivos activo, utilizando un nuevo nombre de archivo y permisos de archivo exclusivos del propietario. Los archivos existentes nunca se sobrescriben.

Presione **/** en el panel de memoria, o **Ctrl-F** desde cualquier panel, para buscar en la memoria. Elija un modo, coincidencia de casos y memoria de proceso legible o la instantánea actual. **Texto** busca UTF-8, UTF-16 (ambos órdenes de bytes) y UTF-32 (ambos órdenes de bytes). **Unicódigo** También normaliza los caracteres de compatibilidad y la combinación de secuencias: por ejemplo, `𝐁𝐨𝐫𝐠` partidos `Borg`. Esto no equivale a letras no relacionadas que simplemente parecen similares. **maleficio** acepta pares de bytes como `42 6F 72 67`.

Las coincidencias ocupan el panel de lista y muestran su dirección, codificación y región originales. Al seleccionar una coincidencia, se obtienen una vista previa de los bytes circundantes en el panel opuesto, con los bytes coincidentes resaltados. Enter enfoca la vista previa; Esc devuelve el foco a los partidos. Esc de coincidencias restaura el mapa anterior o la lista de procesos. La búsqueda de una instantánea almacenada utiliza sus bytes capturados sin leer el proceso en ejecución. Las búsquedas de procesos se leen en fragmentos, informan bytes inaccesibles y se detienen después de 2000 coincidencias o 60 segundos. **norte** continúa desde esa posición; **Ctrl-C** se detiene preservando los resultados. Los informes de cobertura distinguen los bytes buscados y los inaccesibles. Un proceso puede cambiar su memoria mientras se busca.

Lecturas de mapas de memoria o instantáneas restringidas y búsquedas parcialmente inaccesibles ofrecen **R: Administrador**. En macOS esto abre el mensaje de autorización del sistema; en Linux usa `pkexec` y necesita un agente de autorización disponible. Sólo se eleva un ayudante de inspección de corta duración. Borg sigue ejecutándose normalmente y el asistente solo devuelve datos del mapa o bytes delimitados. macOS protecciones de proceso y Linux política de seguridad aún pueden denegar el acceso; Borg muestra la falla y deja utilizable el monitor de proceso ordinario.

La identidad del proceso incluye su hora de inicio, por lo que un PID reutilizado no hereda el historial de otro proceso. Los mapas de memoria, los resultados de la inspección, las instantáneas y el historial están limitados. Borg Commander permanece visible en la esquina inferior derecha, incluida la consola, el visor, el monitor, los cuadros de diálogo y el protector de pantalla.
