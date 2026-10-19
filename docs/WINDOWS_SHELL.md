# « Coucou Shell » : la couche Windows de Coucou

État : **phases 1 à 4 et mode « barre du haut » faits** — le panneau et ses 8 onglets (Contrôles, Aujourd'hui, Stats, Presse-papiers, Apps, Fonds, Alimentation, Fenêtres), désactivé par défaut dans Réglages → Général, les outils vocaux de « OK Coucou » à niveaux de risque (`shell/voice_tools.rs`, cartes de niveau 2 dans `shell/confirm.rs` et `views/confirm.ts`), et les routines (`shell/routines.rs` : liste d'étapes de niveau 1 figée, validée sur une carte, rejouée sans LLM, listée et supprimable dans Réglages → Général), et « Entoure et demande » (`shell/capture.rs`, `circle.html`, `views/circle.ts` : raccourci Ctrl+Alt+X désactivé par défaut ou bouton lasso du panneau, capture GDI en mémoire, aperçu, puis le chat de Claude avec l'image au premier message), et le mode « barre du haut » (`shell/appbar.rs` : masquage automatique par `ABM_SETSTATE`, état d'origine écrit avant le changement et remis à la désactivation, à la sortie et au démarrage suivant après un plantage, raccourci de secours Ctrl+Alt+B enregistré seulement dans ce mode, réserve du haut en AppBar sur l'écran principal, horloge dans l'island repliée). Les mesures sur ta machine (sonde et Mica) restent à faire.

Ce document part de la spécification de ton prompt et des API Windows documentées par Microsoft, et seulement de là. Aucun produit tiers n'a été téléchargé, analysé ni cherché (clean room). Le design reste celui de Coucou et suit les règles de `CLAUDE.md`.

## Architecture proposée

- **Pas de nouvelle fenêtre permanente.** Les panneaux sont un nouvel état « déplié » de la fenêtre de l'island existante (`windows/src/island/`), comme l'est déjà la vue chat. Pas de second WebView, donc pas de RAM en plus quand le panneau est fermé.
- **Une seule fenêtre à la demande** : l'overlay plein écran de « Entoure et demande », créé au raccourci et détruit après la capture.
- **Rust** : un module `windows/src-tauri/src/shell/` avec un sous-module par domaine (`controls`, `stats`, `windows_mgr`, `apps`, `wallpaper`, `power`, `clipboard`, `appbar`, `routines`, `capture`). Chacun expose des commandes Tauri pour les panneaux et des fonctions pour les outils vocaux. Le code Windows reste dans ces modules ; sous Linux, les panneaux qui n'ont pas d'équivalent sont masqués.
- **Outils vocaux** : ils s'ajoutent au serveur MCP local existant (`claude_cli/mcp.rs`). Leur niveau de risque est décidé **en Rust, dans une table fixe**, jamais par le modèle.
- **Coût** : zéro échantillonnage quand le panneau est fermé. Ouvert, Stats lit à 1 Hz (la liste des processus toutes les 2 s). Le presse-papiers est écouté par `WM_CLIPBOARDUPDATE`, sans polling.

## Carte des API, fonctionnalité par fonctionnalité

« Documentée » veut dire une API Win32/WinRT décrite sur learn.microsoft.com. Les features sont celles de la crate `windows` (0.61) qu'il faudrait ajouter à `windows/src-tauri/Cargo.toml`, en plus de celles déjà présentes.

| Fonction | API | Documentée | Features à ajouter |
| --- | --- | --- | --- |
| Volume, muet | `IMMDeviceEnumerator` → `IAudioEndpointVolume` | ✓ | `Win32_Media_Audio_Endpoints`, `Win32_System_Com_StructuredStorage` |
| Choisir la sortie audio | aucune (seul `IPolicyConfig`, non documenté) | **✗** | — |
| Wi-Fi, Bluetooth | WinRT `Windows.Devices.Radios` (`RequestAccessAsync`, `SetStateAsync`) | ✓ | `Devices_Radios` |
| Luminosité, écran externe | DDC/CI : `GetPhysicalMonitorsFromHMONITOR`, `Get/SetMonitorBrightness` | ✓ | `Win32_Devices_Display` |
| Luminosité, écran d'un portable | WMI `WmiMonitorBrightnessMethods` | ✓ (WMI) | `Win32_System_Wmi` |
| Éclairage nocturne | aucune | **✗** | — |
| Thème clair/sombre | lien vers `ms-settings:colors` (retenu) | ✓ | déjà là |
| CPU global | `GetSystemTimes` | ✓ | déjà là |
| RAM | `GlobalMemoryStatusEx` | ✓ | déjà là |
| Réseau | `GetIfTable2` | ✓ | `Win32_NetworkManagement_IpHelper`, `Win32_NetworkManagement_Ndis` |
| GPU | PDH `\GPU Engine(*engtype_3D)\Utilization Percentage` | ✓ | `Win32_System_Performance` |
| Top 5 processus | `CreateToolhelp32Snapshot`, `GetProcessTimes`, `K32GetProcessMemoryInfo` | ✓ | `Win32_System_ProcessStatus` |
| Liste des fenêtres | `EnumWindows` + `DwmGetWindowAttribute(DWMWA_CLOAKED)` | ✓ | `Win32_Graphics_Dwm` |
| Focus, réduire, agrandir, déplacer | `SetForegroundWindow`, `ShowWindow`, `SetWindowPos`, `MonitorFromWindow` | ✓ | déjà là |
| Snap moitié/quart | pas d'API Snap Layouts ; rectangle calculé sur la zone de travail + `SetWindowPos` | ✓ (par calcul) | — |
| Lanceur d'apps | `SHGetKnownFolderItem(FOLDERID_AppsFolder)` + `IEnumShellItems` ; lancement par `shell:AppsFolder\<AUMID>` | ✓ | `Win32_UI_Shell_Common` |
| Fonds d'écran | `IDesktopWallpaper` ; images déposées par l'utilisateur, copiées dans le dossier de Coucou | ✓ | déjà là (`Win32_UI_Shell`) |
| Verrouiller, veille, redémarrer, éteindre | `LockWorkStation`, `SetSuspendState`, `ExitWindowsEx` (+ privilège `SE_SHUTDOWN_NAME`) | ✓ | `Win32_System_Power`, `Win32_System_Shutdown` |
| Presse-papiers | `AddClipboardFormatListener`, `WM_CLIPBOARDUPDATE`, formats `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` | ✓ | `Win32_System_DataExchange` |
| Taper du texte | `SendInput` (déjà utilisé par Flow) | ✓ | déjà là |
| Barre des tâches masquée | `SHAppBarMessage(ABM_SETSTATE, ABS_AUTOHIDE)` | ✓ | déjà là |
| Réserver le haut | `ABM_NEW`, `ABM_QUERYPOS`, `ABM_SETPOS`, `ABN_POSCHANGED`, `ABM_REMOVE` | ✓ | déjà là |
| Icônes de la zone de notification | aucune pour lire celles des autres apps | **✗** | — |
| Capture d'une zone | `BitBlt` depuis l'écran (simple), ou `Windows.Graphics.Capture` (`CreateForMonitor`) | ✓ | `Win32_Graphics_Gdi` (déjà là) |
| Raccourci maintenu | `tauri-plugin-global-shortcut` donne `Pressed` / `Released` | ✓ | déjà là |
| Mica / Acrylic | `windowEffects` de Tauri 2 (sans nouvelle crate) ; en dessous, `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` | ✓ | déjà là |
| Animations réduites | `SystemParametersInfo(SPI_GETCLIENTAREAANIMATION)`, et `prefers-reduced-motion` côté WebView | ✓ | déjà là |

## Hypothèses du prompt qui ne tiennent pas (je m'arrête dessus)

1. **Choisir la sortie audio** : Windows n'a pas d'API documentée pour changer le périphérique par défaut. **Décidé** : le volume et le muet (documentés), la sortie en cours affichée, et un lien vers `ms-settings:sound`.
2. **Éclairage nocturne** : aucune API ; son état est un blob non documenté du registre. **Décidé** : un lien vers `ms-settings:nightlight`.
3. **Thème clair/sombre** : pas d'API non plus, seulement deux valeurs du registre. **Décidé** : un lien vers `ms-settings:colors`, sans écriture dans le registre.
4. **Luminosité** : documentée, mais par deux chemins. DDC/CI pour un écran externe (tous ne le gèrent pas), WMI pour l'écran d'un portable. Le spike dit ce que ta machine supporte.
5. **Snap** : Snap Layouts n'a pas d'API. Les moitiés et les quarts seront calculés sur la zone de travail et appliqués avec `SetWindowPos`. Le rendu sera le même, mais sans l'animation de Windows.
6. **Icônes de la zone de notification** : impossible proprement. Ce sera documenté comme limite, comme ton prompt le prévoit.
7. **Fonds Unsplash** : leur API exige une clé et des règles d'attribution. **Décidé** : pas d'Unsplash ni d'appel réseau. L'utilisateur glisse sa propre sélection d'images dans l'onglet.
   - Seuls les chemins d'un vrai dépôt du système sont acceptés, comme pour l'inbox actuelle (`files.rs`).
   - Le fichier est vérifié comme image (JPG, PNG ou WebP par ses premiers octets) et sa taille est plafonnée.
   - Il est copié dans `%LOCALAPPDATA%\Coucou\wallpapers`, puis appliqué avec `IDesktopWallpaper`.
8. **Météo sans clé** : Open-Meteo marche sans clé. Mais `CLAUDE.md` dit « appels réseau seulement vers les services configurés par l'utilisateur ». **Décidé** : désactivée par défaut, une ville saisie à la main (pas de géolocalisation), au plus un appel toutes les 30 minutes et seulement quand l'onglet Aujourd'hui est ouvert.
9. **Mica sur l'island** :
   - Mica n'existe que sous Windows 11 (22H2+ pour le réglage par fenêtre). Sous Windows 10, il n'y a qu'Acrylic, qui saccade connu lors des déplacements et redimensionnements, et l'island change de taille en s'animant.
   - Il faut aussi vérifier que le WebView transparent laisse voir le fond DWM.
   - Le spike le teste sur ta machine. Si c'est raté, il reste le repli prévu : un fond semi-opaque avec un léger dégradé.
10. **`type_text` dans une fenêtre administrateur** : Windows bloque `SendInput` vers une app élevée si Coucou ne l'est pas (UIPI). C'est une limite, pas un contournement à chercher.
11. **Passer une image au CLI** : vérifié pendant le mode abonnement, sans `Read`. Un bloc `image` en base64 dans le message `--input-format stream-json` passe tel quel (test : « l'image est rouge »). « Entoure et demande » n'écrit donc **aucun fichier**.

## Sécurité

### Niveaux de risque (table fixe en Rust)

| Niveau | Outils | Ce que fait Coucou |
| --- | --- | --- |
| 0 Lecture | `system_stats`, `list_windows`, `list_apps`, `clipboard_recent` (5 derniers, 500 caractères ; seulement si l'historique **et** « OK Coucou peut le lire » sont activés, désactivé par défaut) | exécute |
| 1 Réversible | `set_volume`, `toggle_wifi`, `toggle_bluetooth`, `open_app`, `focus_window`, `snap_window`, `apply_layout`, `set_wallpaper`, `web_open` | exécute, puis l'annonce à voix haute |
| 2 Confirmation | `power_action`, `type_text`, `create_routine`, `schedule_action` | **n'exécute pas** : renvoie au modèle « une carte de confirmation est affichée » et pose une carte dans l'island. Seul le clic exécute |

- **Niveau 2** : le résultat de l'outil ne contient jamais « fait ». L'action n'existe que dans la carte, et la carte ne s'exécute que sur un clic. Pour l'alimentation, le « oui » vocal ne fait qu'afficher la carte, sans cliquer dessus.
- **`web_open`** : seulement une URL de recherche construite par Coucou sur un domaine de l'allowlist (YouTube, Google, Amazon…). Le modèle donne les mots, jamais l'URL.
- **`open_app`** : seulement une app trouvée dans le menu Démarrer ou `shell:AppsFolder`, jamais un chemin.
- **`type_text`** : tape le texte et n'envoie jamais Entrée. Les caractères de contrôle sont retirés, et la longueur est limitée.
- **Interdits, sans outil pour les faire** : shell, exécutable par chemin, fichiers hors des dossiers de Coucou, envoyer, valider ou payer à la place de l'utilisateur.
- **Les 4 couches existantes** s'appliquent : chaque nouveau nom entre dans `hook/src/tool_lists.rs`, ce qui sert à la fois `build_args`, `permissions.allow`, le hook de garde et le coupe-circuit, avec des tests.
- **Téléphone** : aucun de ces outils (`Mode::Phone` reste sans outil).

### Routines

- À la création, Claude traduit la demande en **liste d'actions figée** (niveaux 0 et 1, avec leurs paramètres). Tu la valides sur une carte.
- Au déclenchement, Coucou l'exécute **sans appeler de LLM**. Une page web ne peut donc pas modifier une routine.
- Stockage en JSON dans le dossier de Coucou, liste visible et supprimable dans les réglages, notification après chaque exécution. Les routines ne tournent que si Coucou tourne.

### Presse-papiers

Désactivé par défaut.
- Historique en mémoire seulement, 25 entrées, texte seul.
- Ignoré : les contenus marqués `ExcludeClipboardContentFromMonitorProcessing` ou `CanIncludeInClipboardHistory = 0`, et les apps d'une liste d'exclusion.
- Effacement en un clic.

### Captures et textes

Jamais dans les logs. La capture d'« Entoure et demande » ne vit qu'en mémoire, le temps de l'aperçu et du tour.

### Barre des tâches

- L'état d'origine est écrit **avant** tout changement, puis restauré quand on désactive l'option, quand Coucou quitte, et au démarrage suivant après un plantage.
- Un raccourci de secours réaffiche la barre.
- Jamais `Shell_TrayWnd` directement, jamais le registre d'Explorer. Si `ABM_SETSTATE` sans fenêtre est ignoré par une version de Windows, la poignée de `Shell_TrayWnd` est seulement cherchée (`FindWindowW`) et passée au même appel documenté ; la fenêtre elle-même n'est jamais touchée.
- La bande réservée est celle de l'écran principal (pas encore celui choisi pour l'island).

## Dictée (E)

Vérifié, sans nouveau moteur : le nettoyage de Flow (`flow/cleanup.rs`) appelle `claude::complete`, qui passe par `claude_cli::complete` quand le mode abonnement est activé (délai porté à 10 s). Sa consigne garde seulement la correction quand on se reprend à l'oral (« à 3 h, non, à 4 h » → « à 4 h »). Dans le panneau, la dictée sert aussi de saisie vocale pour la question de « Entoure et demande » : Flow colle le texte dans le champ du chat.

## Mémoire et CPU (budget)

- **Panneau fermé** : 0 % de CPU, aucun timer, aucun listener actif (sauf le presse-papiers s'il est activé).
- **Panneau ouvert** : moins de 3 % de CPU. Les graphiques sont dessinés en `<canvas>` uniquement à chaque nouvel échantillon, sans animation continue, et en 60 fps pendant le dépliage seulement.
- **RAM** : pas de second WebView, les icônes d'apps sont mises en cache en petit (32 px) et chargées à l'ouverture de l'onglet Apps.

## Spikes à lancer sur ton PC

### 1. Sonde des API (lecture seule)

```powershell
cd windows\spikes\shell-spike
cargo run --release > shell-spike-report.md
```

Elle ne modifie rien : pas de volume, de radio, de fond d'écran, de barre des tâches ni de fenêtre, et elle ne lit pas le contenu du presse-papiers. Elle rapporte :
- la version de Windows ;
- le volume et le nombre de sorties ;
- les radios (Wi-Fi, Bluetooth) ;
- chaque écran (taille, DPI, luminosité DDC/CI) ;
- le thème ;
- le nombre de fenêtres et le temps de l'énumération ;
- le nombre d'apps de `shell:AppsFolder` et le temps de la lecture ;
- les fonds d'écran par écran ;
- l'état de la barre des tâches ;
- les formats d'exclusion du presse-papiers ;
- **le coût d'un échantillonneur Stats à 1 Hz pendant 20 s** (CPU, RAM, réseau, GPU, top 5) : temps par échantillon, et % de CPU d'un cœur et de la machine.

Vérifiée par `cargo check` et `cargo clippy` pour `x86_64-pc-windows-msvc`, sans avertissement. Elle n'a pas été exécutée sous Windows ici.

### 2. Mica / Acrylic sur l'island (5 min, rien n'est committé)

1. Dans `windows/src-tauri/tauri.conf.json`, fenêtre `island`, ajouter temporairement `"windowEffects": { "effects": ["mica"] }`, puis `["acrylic"]`.
2. Dans `windows/src/styles/island/island.css`, mettre temporairement `#island { background: rgb(var(--color-scrim-rgb) / 0.55) }`.
3. `npm run tauri dev`, ouvrir et fermer l'island, la déplacer, regarder le Gestionnaire des tâches (CPU de `coucou.exe` et de `msedgewebview2.exe`).
4. Noter pour chaque effet et chaque version de Windows : le rendu (le flou se voit-il à travers le WebView ?), la saccade à l'ouverture, et le CPU au repos comme pendant l'animation.
5. Annuler les deux changements (`git checkout -- windows/src-tauri/tauri.conf.json windows/src/styles/island/island.css`).

### 3. Passage d'image au CLI

Déjà fait (point 11 ci-dessus).

## Maquettes

Un canevas (privé, ouvrable dans le navigateur) : https://claude.ai/artifact/W9FfVr8jyjebUjqXi4EDth. Il contient :
- l'island fermée, en train de se déplier, et en mode « barre du haut » ;
- les 8 onglets en sombre ;
- la carte de confirmation (niveau 2) et la carte de création de routine ;
- l'overlay « Entoure et demande » ;
- Contrôles et Stats en clair.

Chaque onglet a deux réglages (bouton Tweaks) : le thème sombre/clair et la couleur d'accent, celle de Mochi.

Les sources sont dans `design/mockups/windows-shell/` (`*.dc.html`, avec l'index `canvas.json`). Elles s'affichent dans ce canevas : ouvertes seules dans un navigateur, il leur manque son moteur de rendu.

Le style suit `windows/src/styles/` et ses couleurs `windows/src/design/tokens.css` (voir `docs/DESIGN_SYSTEM.md`) :
- l'island noire, les cartes à coins de 20 px et les filets à faible opacité ;
- Segoe UI Variable, et Cascadia Mono en chiffres tabulaires ;
- une grille de 8 px ;
- les cartes de niveau 2 reprennent la carte d'approbation existante (Mochi avec pastille « ! », boutons pilule, bouton principal blanc).

Mochi est dessiné en code : il dort sur l'onglet Alimentation et regarde l'onglet actif. Les icônes sont des tracés simples, faits à la main dans le style « trait » de Lucide. Les photos de fonds d'écran sont des dégradés de remplacement, avec « [Photographe] » à la place des crédits.

## Décisions (18 octobre)

1. **Direction visuelle** : validée. Dans l'island repliée (mode « barre du haut »), l'heure et une date courte (« sam. 18 ») ; la date complète au survol. L'island prend la largeur de son contenu, sans barre de séparation.
2. **Thème** : un lien vers les Paramètres.
3. **Fonds d'écran** : la sélection de l'utilisateur, déposée dans l'onglet.
4. **Météo** : Open-Meteo, en option, avec une ville saisie.
5. **Sortie audio et éclairage nocturne** : des liens vers les Paramètres.
6. **Branche** : `feat/windows-shell`, une PR à part qui part de la PR 1 (mode abonnement).

## Écarts avec les maquettes (PDF « Coucou Shell — maquettes »)

Les onglets suivent les pages sombres du PDF (tailles, rayons, interrupteurs 48×28, cartes, pieds de page, carte de confirmation). Ce qui ne peut pas être identique :

- **Mode avion** : Windows n'expose pas le mode avion dans l'API des radios. La carte ouvre la page Réseau des Paramètres au lieu d'un interrupteur.
- **Alimentation** : quatre tuiles comme la maquette. « Se déconnecter » reste une action (sa carte de confirmation existe) mais n'a pas de tuile.
- **Apps** : la maquette dit « Épinglées ». L'app n'a pas d'épingles ; la grille montre les apps lancées récemment. Pas de sous-titre « Application » ou « Store » : la liste de Windows ne le donne pas.
- **Stats** : pas de nom de processeur ni de carte graphique (`ShellStats` ne les fournit pas), donc pas de sous-titre sous CPU et GPU.
- **Presse-papiers** : texte en police normale, pas en police à chasse fixe pour le code.
- **Clair** : les maquettes claires (`ControlesClair`, `StatsClair`) ne sont pas faites. L'island reste sombre, comme dans l'app.
- **Carte de confirmation** (saisie, routine) : elle garde le style de la carte d'approbation de `main`, qui ne se retouche pas sans demande.
- **« Entoure et demande »** : la maquette affiche Ctrl+Maj+Espace ; le raccourci de l'app est Ctrl+Alt+X.

## Mesures

À remplir avec `shell-spike-report.md` et le test Mica.
