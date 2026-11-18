# Consumo de recursos al editar `.env`

Prueba local del 17 de noviembre de 2026 con **Dotvar 0.1.0** y **Visual Studio Code 1.146.0**, ambos arm64, ejecutados desde sus bundles instalados en `/Applications`. Equipo Mac mini M2 (Mac14,3, 8 núcleos, 24 GiB), macOS 27.0.1 (26A434), conectado a la corriente.

## Resultados

En ambos tamaños de archivo, Dotvar usó **93 % menos memoria en reposo** y registró un **pico de memoria 92 % menor durante la edición**. Para esta comparación, VS Code se ejecutó con un perfil limpio, sin extensiones de usuario y con la IA integrada desactivada.

| Archivo | App | Memoria en reposo | Pico de memoria durante la edición | CPU en reposo | CPU total de tres ediciones | Procesos en reposo |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 200 variables | Dotvar | 56,2 MiB | 69,8 MiB | 0,0004 % | 0,030 s | 1 |
| 200 variables | VS Code | 805,2 MiB | 891,0 MiB | 0,0039 % | 0,065 s | 7 |
| 5.000 variables | Dotvar | 58,8 MiB | 73,8 MiB | 0,0005 % | 0,033 s | 1 |
| 5.000 variables | VS Code | 849,6 MiB | 883,9 MiB | 0,0041 % | 0,066 s | 7 |

Cada celda es la mediana de tres aperturas independientes. El pico es el máximo observado en cada corrida. La CPU en reposo se expresa como porcentaje de un núcleo y la CPU de edición como tiempo acumulado de usuario y sistema.

En las tres búsquedas, cambios y guardados, Dotvar acumuló **54 % menos tiempo de CPU** con 200 variables y **50 % menos** con 5.000. La CPU en reposo fue muy baja en ambas apps.

| Espacio del bundle instalado | Dotvar | VS Code | Reducción |
| --- | ---: | ---: | ---: |
| Bloques ocupados según `du -sk` | 29,9 MiB | 1.004,9 MiB | 97,0 % |

### VS Code con la IA integrada habilitada

En tres corridas con 200 variables y la IA habilitada, VS Code registró 944,0 MiB en reposo, 1.004,0 MiB de pico durante la edición, 0,0108 % de un núcleo en reposo y 0,070 s de CPU en las tres ediciones. La mediana fue de 10 procesos en reposo. Se mantuvo la configuración temporal y se usó sin iniciar sesión.

## Método

1. Abrir un único `.env` sintético de **200 variables** (6.624 bytes) o **5.000 variables** (165.024 bytes) en una ventana de 1.281 × 832 puntos. Arrancar un proceso nuevo con un perfil temporal por corrida. Alternar apps y tamaños con las cachés del sistema en uso.
2. Ocultar la app con el archivo abierto, esperar 7 segundos y tomar muestras durante 12 segundos sin interacción. Usar la mediana de memoria y el incremento de CPU en ese intervalo.
3. Volver a poner la app al frente. Buscar con ⌘F las claves al 25 %, 50 % y 75 % del archivo, reemplazar el valor y guardar con ⌘S después de cada cambio. En Dotvar se revela solo el valor encontrado. En VS Code se selecciona el valor en el editor de texto después de cerrar la búsqueda.
4. Comparar el archivo entero con los bytes esperados después de cada guardado. Las tres ediciones son acumulativas. Muestrear la fase activa desde que la ventana vuelve al frente hasta 350 ms después de confirmar el último guardado.
5. Muestrear cada 250 ms. Para cada PID del árbol de la app, leer `ri_phys_footprint`, `ri_user_time` y `ri_system_time` mediante `proc_pid_rusage`. Sumar la huella física simultánea de los procesos y el incremento de CPU. En VS Code se incluyen los procesos auxiliares del editor. La huella reportada es la suma por proceso y su unidad es MiB.

En Dotvar se usó un catálogo temporal mediante `DOTVAR_CONFIG_DIR` y se conservaron los valores ocultos por defecto. En VS Code se usaron directorios temporales con `--user-data-dir`, `--extensions-dir` y `--shared-data-dir`, junto con `--disable-extensions`. Se desactivaron telemetría, actualizaciones y experimentos, y se omitieron la bienvenida, la restauración y la confianza del workspace. En la comparación principal también se configuró `"chat.disableAIFeatures": true`.

El alcance de estas cifras es la memoria y la CPU de los procesos medidos en esta Mac para una sola ventana y un solo archivo, con las demás aplicaciones de la sesión abiertas. El espacio en disco corresponde al bundle de cada app. Los resultados de edición incluyen la búsqueda, la selección, la escritura, el guardado y las pausas de la automatización.

### Archivo de prueba

Para regenerar el contenido inicial en una carpeta temporal, usá `count = 200` o `count = 5000`.

```python
from pathlib import Path
from tempfile import mkdtemp

count = 200
fixture = "# Synthetic environment\n" + "".join(
    f"BENCH_{index:05d}=value_{index:05d}_baseline\n"
    for index in range(1, count + 1)
)
path = Path(mkdtemp(prefix="dotenv-benchmark-")) / ".env"
path.write_bytes(fixture.encode("utf-8"))
print(path)
```

En cada una de las tres claves, el valor esperado termina en `_changedx` en lugar de `_baseline`.

### Corridas individuales

Las 15 corridas válidas confirmaron los 45 guardados contra el archivo esperado. Se repitieron las corridas interrumpidas y las que fallaron la verificación.

| Archivo | App | Corrida | Memoria en reposo (MiB) | Pico de edición (MiB) | CPU en reposo (% de un núcleo) | CPU de edición (s) | Procesos en reposo |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 200 | Dotvar | 1 | 56,2 | 69,9 | 0,0004 | 0,029 | 1 |
| 200 | Dotvar | 2 | 51,3 | 69,8 | 0,0004 | 0,030 | 1 |
| 200 | Dotvar | 3 | 56,7 | 69,4 | 0,0005 | 0,031 | 1 |
| 200 | VS Code | 1 | 636,1 | 770,1 | 0,0053 | 0,062 | 7 |
| 200 | VS Code | 2 | 819,1 | 930,7 | 0,0039 | 0,067 | 7 |
| 200 | VS Code | 3 | 805,2 | 891,0 | 0,0039 | 0,065 | 7 |
| 200 | VS Code (IA habilitada) | 1 | 820,5 | 946,8 | 0,0108 | 0,070 | 10 |
| 200 | VS Code (IA habilitada) | 2 | 944,0 | 1.004,0 | 0,0107 | 0,081 | 10 |
| 200 | VS Code (IA habilitada) | 3 | 989,0 | 1.053,9 | 0,0111 | 0,070 | 10 |
| 5.000 | Dotvar | 1 | 52,7 | 72,9 | 0,0006 | 0,032 | 1 |
| 5.000 | Dotvar | 2 | 58,9 | 73,8 | 0,0005 | 0,033 | 1 |
| 5.000 | Dotvar | 3 | 58,8 | 73,9 | 0,0004 | 0,033 | 1 |
| 5.000 | VS Code | 1 | 830,2 | 925,6 | 0,0041 | 0,079 | 7 |
| 5.000 | VS Code | 2 | 849,6 | 872,4 | 0,0041 | 0,066 | 7 |
| 5.000 | VS Code | 3 | 855,8 | 883,9 | 0,0041 | 0,066 | 7 |

## Texto sugerido para el README

```markdown
In a local benchmark on an M2 Mac mini, Dotvar 0.1.0 used **93% less memory** than VS Code 1.146.0 with a single `.env` file open in the background (200 and 5,000 variables). The installed Dotvar app used **97% less disk space**. VS Code ran with a clean profile, user extensions disabled, and integrated AI off. [Benchmark results and methodology](docs/resource-benchmark.md).
```
