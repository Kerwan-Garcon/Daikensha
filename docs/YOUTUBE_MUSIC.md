# YouTube Music dans Coucou

YouTube Music n'a pas d'app de bureau officielle (Google ne propose que la version web et les apps mobiles), et la version web n'expose ni AppleScript ni API. Coucou passe donc par **[Pear Desktop](https://github.com/pear-devs/pear-desktop)** (anciennement `th-ch/youtube-music`) : un client YouTube Music de bureau open source (MIT), disponible sur Mac, Windows et Linux. Son plugin **API Server** expose une API locale.

La pill `integration_youtube_music` affiche la même carte et la même pill que Spotify : pochette, titre, artiste · album, barre de progression (glisser pour avancer), lecture/pause, précédent/suivant, aléatoire, répétition (tout / un) et volume. Mochi danse pendant la lecture.

| Plateforme | Code | Disponibilité |
|---|---|---|
| macOS | `NotchBuddy/Sources/App/YouTubeMusicController.swift`, `YouTubeMusicViews.swift` | build GitHub seulement (`githubOnly`), comme Spotify |
| Windows et Linux | `windows/src-tauri/src/youtube_music.rs`, `windows/src/core/youtube-music.ts`, `windows/src/island/youtube-music.ts`, `windows/src/views/youtube-music.ts` | les deux, même code |

## Comment ça marche

- **Adresse** : `http://127.0.0.1:26538` (le port par défaut du plugin), jamais une autre adresse et jamais à travers un proxy.
- **Premier branchement** : la carte affiche « Autorisez Coucou à piloter YouTube Music » avec un bouton **Connecter…**. Le clic envoie `POST /auth/coucou`, Pear affiche alors sa propre boîte « Autoriser / Refuser ». Rien n'est accepté à la place de l'utilisateur.
- **Jeton** : le JWT renvoyé par Pear est rangé dans le Trousseau (Mac, clé `youtube-music-token`), le Gestionnaire d'identification (Windows) ou le Secret Service (Linux), dans les clés internes que la webview ne peut pas lire. S'il n'est plus valide (`401` sur une commande, ou fermeture du WebSocket avec le code `1008`), il est effacé et la carte redemande **Connecter…**.
- **État** : un WebSocket `ws://127.0.0.1:26538/api/v1/ws?token=…` reçoit `PLAYER_INFO`, puis un message par changement (`VIDEO_CHANGED`, `PLAYER_STATE_CHANGED`, `POSITION_CHANGED`, `VOLUME_CHANGED`, `REPEAT_CHANGED`, `SHUFFLE_CHANGED`). Pear envoie la position toutes les secondes pendant la lecture. Coucou ne redessine que si elle saute de plus de 1,5 s (un seek), sinon la carte extrapole comme pour Spotify.
- **Commandes** : `POST /api/v1/{toggle-play, play, pause, next, previous, seek-to, volume, shuffle, switch-repeat}` avec `Authorization: Bearer`. YouTube Music fait tourner la répétition dans l'ordre AUCUNE → TOUT → UN : activer = 1 appui, couper depuis TOUT = 2 appuis, couper depuis UN = 1 appui.
- **Pochettes** : uniquement depuis les CDN Google (`*.googleusercontent.com`, `*.ytimg.com`, `*.ggpht.com`) en https.
- **Pear absent** : sur Mac, Coucou écoute le lancement et la fermeture de Pear (bundle `com.github.th-ch.youtube-music`), sans aucun minuteur. Sur Windows et Linux, une tentative de connexion à 127.0.0.1 est faite après 2, 4, 8… secondes, jusqu'à une par minute. Une tentative est aussi faite tout de suite quand la carte s'affiche et après **Ouvrir YouTube Music**.

## Installer côté utilisateur

1. Installer Pear Desktop depuis ses [releases](https://github.com/pear-devs/pear-desktop/releases/latest) (Mac : le `.dmg` ; Windows : `YouTube-Music-Web-Setup-<version>.exe`, qui télécharge et installe l'app dans `%LOCALAPPDATA%\Programs\youtube-music` ; Linux : `.deb`, `.rpm`, AppImage ou AUR). Sur Mac, l'app n'est pas signée : ouvrez-la une première fois par clic droit → Ouvrir.
2. Dans Pear : menu **Plugins → API Server** → activer. Laisser l'authentification sur « Auth at first » (par défaut). Conseillé : régler **Hostname** sur `127.0.0.1` (par défaut `0.0.0.0`, l'API est alors joignable depuis le réseau local).
3. Dans Coucou : Réglages → pills actives → **YouTube Music**.
4. Ouvrir la carte YouTube Music → **Connecter…** → **Autoriser** dans Pear.

## Plan de test

### 0. Tests automatiques (à lancer avant tout)

| Quoi | Commande | Attendu |
|---|---|---|
| Logique Rust (messages Pear, commandes, répétition, pochettes, retries) + vrai client WebSocket contre un faux serveur (connexion, 1008, arrêt, Pear absent) | `cd windows/src-tauri && cargo test --lib youtube_music` | 16 tests OK |
| Suite Rust complète (dont la liste des clés internes) | `cd windows/src-tauri && cargo test --lib` | tout vert |
| Page Windows/Linux : états de la carte, contrôles envoyés à Pear et non à Spotify, danse, pochette, renommage de la pill, catalogue | `cd windows && npm test` | tout vert (dont `tests/youtube-music.test.mjs` et `tests/spotify.test.mjs`, qui vérifie que la carte Spotify n'a pas bougé) |
| Types et traductions | `cd windows && npx tsc --noEmit && node scripts/gen-strings.mjs --check` | aucune erreur |
| Build Mac | `cd NotchBuddy && xcodegen && xcodebuild -scheme NotchBuddy -configuration Debug build`, puis le schéma `CoucouAppStore` | les deux compilent (YouTube Music est absent du build App Store) |

La CI `Build` (macOS) et `Windows CI` lancent ces builds sur une pull request vers `main`.

### 1. Sans Pear : le faux serveur

`scripts/fake-pear.mjs` imite le plugin API Server (mêmes routes, boîte « Autoriser », WebSocket, 3 morceaux), avec Node seul :

```
node scripts/fake-pear.mjs          # demande « Allow? » dans le terminal
node scripts/fake-pear.mjs --deny   # refuse tout
```

Pendant qu'il tourne, taper `p` (lecture/pause), `n` (suivant), `s` (aller à 1:00), `v` (volume 30), `r` (répétition), `f` (aléatoire), `x` (oublier les jetons), `q` (quitter, comme Pear qu'on ferme). Le terminal affiche chaque commande reçue de Coucou.

> Sur Mac, le lancement et la fermeture ne sont détectés que pour le vrai Pear (bundle id). Avec le faux serveur, Coucou sonde le port à chaque affichage de la carte : ouvrez la carte (ou repassez dessus) après avoir lancé ou coupé le serveur. Sur Windows et Linux, rien à faire de plus.

| # | Étape | Attendu |
|---|---|---|
| 1.1 | Pill YouTube Music activée, aucun serveur | Carte : « YouTube Music n'est pas installé » + **Installer Pear Desktop** (ou, si Pear est installé : « Activez le plugin API Server… » + **Ouvrir YouTube Music** sur Windows/Linux, « Pas en lecture » sur Mac). Réglages : « Non installé » si Pear est absent |
| 1.2 | Lancer `fake-pear.mjs`, ouvrir la carte | « Autorisez Coucou à piloter YouTube Music » + **Connecter…** |
| 1.3 | **Connecter…** | La carte passe à « Cliquez sur Autoriser dans YouTube Music », sans bouton. Le terminal demande `Allow "coucou"…?` |
| 1.4 | Répondre `n` | « YouTube Music a refusé la connexion » + **Réessayer** |
| 1.5 | **Réessayer**, répondre `y` | Carte en lecture : *Get Lucky*, « Daft Punk · Random Access Memories » (titre nettoyé, sans « (Official Audio) » ni « ft. ») ; la pochette arrive ; la pill porte le titre ; Mochi danse |
| 1.6 | Lecture/pause, suivant, précédent dans la carte | Le terminal affiche `POST toggle-play`, `next`, `previous`, et la carte suit (titre, temps) |
| 1.7 | Glisser la barre de progression | `POST seek-to {"seconds":…}`, la barre reste où on l'a lâchée |
| 1.8 | Glisser le volume | des `POST volume` espacés d'au moins 120 ms, pas un par pixel |
| 1.9 | Aléatoire, puis répétition deux fois | `POST shuffle` ; `switch-repeat {"iteration":1}` puis `{"iteration":2}` ; l'icône passe en rouge puis revient |
| 1.10 | Dans le terminal : `p`, `n`, `s`, `v`, `r`, `r` | La carte suit chaque action faite « dans Pear » ; avec `r` deux fois (mode UN), l'icône devient « répéter 1 » |
| 1.11 | Survoler la pill (carte d'une autre pill au premier plan) | Lecture/pause et suivant apparaissent et fonctionnent |
| 1.12 | `x` dans le terminal | Le WebSocket est fermé avec 1008 → la carte redemande **Connecter…**, le jeton a disparu du Trousseau / Gestionnaire d'identification |
| 1.13 | Reconnecter, puis `q` (Pear quitte) | La carte repasse à l'état de repos, la pill reprend le nom « YouTube Music », Mochi arrête de danser |
| 1.14 | Relancer `fake-pear.mjs --allow` sans toucher à Coucou | Windows/Linux : la carte se reconnecte seule en moins d'une minute (tout de suite si on l'ouvre). Mac : au prochain affichage de la carte. Le faux serveur oublie ses jetons en quittant : **Connecter…** est redemandé, ce qui est normal ici (le vrai Pear les garde) |
| 1.15 | Île cachée, lecture qui démarre (`p`) | L'île compacte apparaît une fois, sans son |
| 1.16 | Désactiver la pill dans les Réglages | Le terminal affiche `WebSocket closed`, plus aucune tentative de connexion ensuite |

### 2. Avec le vrai Pear Desktop

| # | Étape | Attendu |
|---|---|---|
| 2.1 | Pear installé, plugin API Server **désactivé**, Pear lancé | Mac : **Connecter…**, puis après le clic « Activez le plugin API Server dans YouTube Music » + **Réessayer**. Windows/Linux : ce message directement, avec **Ouvrir YouTube Music** |
| 2.2 | Activer le plugin, puis **Réessayer** (Mac) ou attendre / rouvrir la carte (Windows/Linux, qui affiche alors **Connecter…**) | Boîte « Autoriser » de Pear → Autoriser → lecture en direct |
| 2.3 | Lire un morceau, une vidéo, un épisode de podcast | Titre, artiste, durée et pochette justes ; la barre avance ; le seek atterrit au bon endroit |
| 2.4 | Changer le volume dans Pear | Le curseur de Coucou suit ; couper le son → 0 |
| 2.5 | Quitter Pear puis le relancer | Mac : reconnexion automatique dans les 2 à 14 s après le lancement. Windows/Linux : dans la minute, ou tout de suite en ouvrant la carte |
| 2.6 | **Ouvrir YouTube Music** (Pear fermé) | Pear démarre (Mac : bundle ; Windows : `%LOCALAPPDATA%\Programs\youtube-music\YouTube Music.exe` ; Linux : `youtube-music` du PATH ou `/opt/YouTube Music`), puis la carte se connecte |
| 2.7 | Dans Pear : Plugins → API Server → changer le *secret* ou retirer le client | Prochaine commande ou reconnexion → **Connecter…** redemandé |
| 2.8 | Redémarrer Coucou | Reconnexion sans redemander l'autorisation (jeton conservé) |

### 3. Non-régression

- **Spotify** : refaire rapidement la carte et la pill Spotify (lecture, seek, volume, aléatoire/répétition, état « non installé »). La carte Windows/Linux de Spotify est désormais partagée avec YouTube Music, son rendu ne doit pas avoir changé. Sur Mac, `shortTitle`/`shortArtist` sont seulement devenus non privés.
- **Les deux en même temps** : pills Spotify et YouTube Music actives, musique sur l'une puis l'autre. Mochi danse dans l'île compacte dans les deux cas ; en vue étendue, seulement si la carte au premier plan est celle qui joue.
- **Build App Store (Mac)** : la pill n'apparaît pas et rien de YouTube Music n'est compilé (`#if !APPSTORE`).

### 4. Performance et sécurité

- **CPU** : pill active, Pear ouvert mais en pause, île cachée → Coucou à ~0 % (Moniteur d'activité / Gestionnaire des tâches). Pendant la lecture, Pear envoie un message par seconde, ce qui reste négligeable, mais l'île n'est pas redessinée pour autant. Pill désactivée → aucune connexion (`netstat`/`lsof -i :26538` vide côté Coucou).
- **Réseau** : `lsof -i -P | grep -i coucou` (Mac) ou Resource Monitor (Windows) → uniquement `127.0.0.1:26538` et, pour les pochettes, les CDN Google.
- **Jeton** : absent de tout fichier de réglages et de `localStorage`. Sur Windows/Linux, la webview ne peut pas l'atteindre : la clé est interne, hors de la liste lue par `secret_present` / `secret_set` (vérifié par `the_phone_links_secrets_cannot_be_reached_from_the_island`).
- **Pochette malveillante** : une URL `imageSrc` hors des CDN Google (par exemple `http://…` ou `https://127.0.0.1/…`) n'est jamais téléchargée (testé dans `covers_only_come_from_googles_cdns`).

## Limites connues

- Le port est fixé à 26538 (celui par défaut du plugin). Si vous l'avez changé dans Pear, remettez 26538.
- « OK Coucou » pilote YouTube Music (lecture, pause, suivant, précédent, volume, et « mets du lofi » qui joue le premier résultat). Les commandes vont au lecteur qui joue ; si rien ne joue et que Pear est connecté, à YouTube Music ; « … sur YouTube Music » le choisit. La recherche prend le premier résultat de YouTube Music, qui n'est pas toujours celui attendu (un artiste ou un album en tête de résultats donne leur premier titre).
- Sur Mac, si Pear est installé sous un autre identifiant que `com.github.th-ch.youtube-music` (fork), la carte sonde quand même le port quand elle s'affiche, mais le lancement de Pear n'est pas détecté automatiquement, et le bouton d'installation s'affiche faute de le trouver.
