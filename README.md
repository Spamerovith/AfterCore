# AfterCore - исходники

Современный счётчик FPS, мониторинг железа и оверлей поверх игр. Написан на Rust с нуля.
Похож на RTSS по назначению, но внутри устроен иначе: **в процессы игр ничего не внедряется**

```
AfterCore/            ← только то, что нужно для запуска (AfterCore.exe)
Исходники/            ← весь код
  build.ps1           ← сборка + копирование exe в ../AfterCore
  crates/
    ac-core/          ← конфиг, профили, топология CPU, affinity/CPU Sets, бинды, Win32-хелперы
    ac-telemetry/     ← ETW-счётчик кадров, статистика (FPS, 1%/0.1% low), PDH, NVML
    ac-overlay/       ← программный рендер (tiny-skia + ab_glyph), прозрачное окно, логотип
    aftercore/        ← лаунчер (eframe/egui), трей, глобальные бинды, бенчмарк
```

## Сборка

```powershell
powershell -ExecutionPolicy Bypass -File .\build.ps1          # release → ..\AfterCore\AfterCore.exe
powershell -ExecutionPolicy Bypass -File .\build.ps1 -Test    # + cargo test --workspace
```

Нужны Rust (stable, MSVC) и Windows 10/11. Windows SDK не нужен: `build.rs` сам рисует иконку и
пишет `.res` с иконкой и манифестом (PerMonitorV2 DPI, UTF-8, Common Controls v6)

Параметры запуска `AfterCore.exe`:

| Ключ | Что делает |
|---|---|
| `--tray` | старт свёрнутым в трей (этот ключ использует автозапуск) |
| `--no-elevate` | не запрашивать права администратора |
| `--selftest [сек]` | проверка счётчика кадров: печатает процессы, которые выводят кадры, и их FPS; пишет `data\selftest.txt` (запускать от администратора) |
| `--desktop-preview` | сразу показать оверлей на рабочем столе |
| `--screenshot <png> --page <страница>` | служебный: сохранить скриншот интерфейса и выйти |

## Как RTSS устроен внутри (разбор через REA)

Бинарники RTSS из `C:\Program Files (x86)\RivaTuner Statistics Server` изучены статически через
REA CLI (`inspect-pe-imports`, `inspect-native-exports`) и по строкам. Внутрь ничего не внедрялось
и не запускалось. Код RTSS не копировался: эти выводы - только ориентир для собственного дизайна

| Что видно | Источник | Вывод |
|---|---|---|
| `RTSSHooks64.dll` экспортирует `InstallRTSSHook`, `RTSSCBTProc`, `InjectProcess`, `RTSSRemoteProc`; импортирует `SetWindowsHookExA`, `CreateRemoteThread`, `WriteProcessMemory`, `VirtualAllocEx` | REA exports `ev_c7e67fc45f6622183…`, imports `ev_273022bba4afe8e8c…` | Внедрение в процессы: глобальный CBT-хук и удалённые потоки |
| Строки `IDXGISwapChain::Present`, `Present1`, `ResizeBuffers`, `IDirect3DDevice9::Present`, `vkQueuePresentKHR`, `wglSwapBuffers`, `ID3D12CommandQueue::ExecuteCommandLists`, `Dynamic offset …` | строки `RTSSHooks64.dll` | Перехват Present всех API (vtable и detour); смещения vtable ищутся динамически через временные устройства (`D3D11CreateDeviceAndSwapChain` в импортах) |
| Импорты `QueryPerformanceCounter`, `CreateWaitableTimerA`, `SetWaitableTimer`, `timeSetEvent` | imports | Время кадра берётся по QPC прямо в хуке Present; ограничитель FPS ждёт на waitable timer |
| `CopyFontTexture`, `GetFontTextureWidth`, `CreateFontA`, `D3DCompile` | exports / imports | OSD рисуется устройством самой игры: GDI-шрифт → атлас-текстура → свои шейдеры |
| `RTSSSharedMemoryV2`, `\\.\PIPE\RTSS_FrametimeV2`, `OpenFileMappingA`/`MapViewOfFile` | строки / imports | Обмен с сервером: shared memory и pipe |
| `RTSSHooksLoader64.exe` (`EnumProcesses`, `CreateFileMappingA`) | REA imports `ev_b91d1f969fabdd7b0…` | 64-битный хост хуков для 32-битного `RTSS.exe` |

Чего статический разбор не показывает: точный порядок вызовов во время работы и то, как RTSS
обходит конфликты с другими оверлеями. Это осталось за рамками разбора

### Чем AfterCore отличается от RTSS

| Задача | RTSS | AfterCore |
|---|---|---|
| Время кадра | QPC внутри перехваченного `Present` | ETW-события `Present_Start` от DXGI (id 42, 55) и D3D9 (id 1) с QPC-штампами; для Vulkan/OpenGL - DxgKrnl (215, 171, 184, 168). ID сверены с манифестами провайдеров на этой системе |
| Оверлей | рисуется внутри игры её же устройством | отдельное окно `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_TOPMOST \| WS_EX_NOACTIVATE`, программный рендер, `UpdateLayeredWindow` |
| Риск с античитами | внедрение DLL | нет внедрения, игра не трогается |
| Скрытие от записи | - | `WDA_EXCLUDEFROMCAPTURE` по выбору |

## Возможности

- **FPS, frametime, avg, max, 1% / 0.1% low** - отдельно по каждому swapchain, с задержкой 0,3 с, чтобы опоздавшие буферы ETW не рисовали ложные пики
- **FPS без генератора кадров**: маркеры NVIDIA Reflex PCL (`PCLStatsTraceLoggingProvider`, GUID `0d216f06-82a6-4d49-bc4f-8f38ae56efab` извлечён из `sl.pcl.dll`, REA `ev_60bed930bbe698d8f908d…`) дают число настоящих кадров игры. Тип генератора (DLSS / FSR / XeSS) определяется по загруженным модулям; если античит не даёт прочитать список модулей, используются файлы игры, и только когда вывод однозначен, иначе подпись «FG». Без Reflex в игре настоящий FPS узнать нельзя - тогда показывается обычный
- **Датчики**: CPU (общая и по потокам, эффективная частота) и GPU-движки/VRAM через PDH;
  для NVIDIA через NVML ещё температура, частоты, потребление и вентилятор; RAM; CPU и RAM процесса игры
- **Оверлей**: 4 раскладки (Карточка с секциями GPU/CPU/RAM / Классика в стиле OSD / Строка / Только FPS), компактный режим, графики внизу (frametime, FPS, GPU, CPU, RAM), каждое ядро CPU с частотой, 9 точек привязки к окну или монитору,
  масштаб с учётом DPI, 6 встроенных тем и свои, любой системный шрифт или свой `.ttf` в `data\fonts`,
  все цвета, пороги цвета FPS, порядок метрик
- **Ядра CPU**: топология из `GetSystemCpuSetInformation` (P/E-ядра, HT, группы L3/CCD), пресеты под
  архитектуру, «Выбрать все» / «Снять» / «Инвертировать», свои наборы, жёсткий (Affinity) или
  мягкий (CPU Sets) режим, приоритет. «Запоминать для этой игры» создаёт правило
- **Профили**: наборы настроек оверлея (переключаются клавишей) + игровые правила по exe
  (профиль, показ оверлея, ядра, режим, приоритет - применяются сами при запуске игры).
  Экспорт и импорт в `data\exports`
- **Бинды**: низкоуровневый хук клавиатуры (WH_KEYBOARD_LL) в процессе AfterCore - работает и в играх с `RIDEV_NOHOTKEYS`, где `RegisterHotKey` молчит; клавиша не перехватывается у игры
- **Бенчмарк**: запись frametime в CSV, итоги AVG / 1% / 0.1% / MIN / MAX / статтеры, автостоп
- **Лаунчер**: своя рамка окна, 4 темы интерфейса, 8 акцентов и свой цвет, шрифт и масштаб
  интерфейса, трей, автозапуск (через Планировщик задач без UAC, если запущен от администратора)
- **Портативность**: настройки лежат в `AfterCore\data` (если папка недоступна для записи - в `%APPDATA%\AfterCore`)

## Античиты

RTSS совместим с античитами потому, что его хук-библиотека подписана сертификатом Authenticode
(REA `inspect-pe-certificates`, `ev_565491af…`: Sectigo Code Signing на имя автора) и давно в
белых списках, плюс в `ProfileTemplates` у 283 из 356 профилей стоит `EnableHooking = 0` для
лаунчеров и компонентов античитов. AfterCore не внедряется вовсе, поэтому вместо этого:

- процесс игры не открывается вообще: путь exe, CPU, RAM, потоки и подкачка берутся из общего
  списка процессов ядра (`NtQuerySystemInformation`), как у Диспетчера задач; в коде нет
  `PROCESS_VM_READ`, `EnumProcessModules`, `VirtualQueryEx`, `ReadProcessMemory`, хуков, внедрения;
- ядра и приоритет у защищённых игр не меняются ни автоматически, ни кнопкой, пока это явно не
  разрешено в настройках (это единственное место, где нужен доступ к процессу);
- распознаются BattlEye, Easy Anti-Cheat, VAC, Riot Vanguard, FACEIT, ESEA, Ricochet, EA Javelin,
  GameGuard, XIGNCODE3, ACE, NetEase, HoYoverse, Denuvo AC, EQU8, Hyperion, PunkBuster
  (по имени exe, файлам рядом с игрой и службам) → безопасный режим: оверлей не прячется от
  скриншотов, ядра/приоритет не меняются без разрешения;
- в exe есть VERSIONINFO. Подписи кода нет - для неё нужен платный сертификат

## Ограничения

- Счётчику FPS (ETW) нужны **права администратора**. Без них работают датчики, ядра и интерфейс
- **Ограничителя FPS нет**: ограничивать кадры можно только изнутри игры, а это требует внедрения
- Внешний оверлей не виден в настоящем эксклюзивном полноэкранном режиме (старый D3D9 или
  выключенная оптимизация полноэкранного режима). В оконном, безрамочном и обычном
  «полноэкранном» режиме Windows 10/11 он работает
- Температура CPU не показывается: для неё нужен драйвер ядра
