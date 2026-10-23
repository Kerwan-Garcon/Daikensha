# Audit du code — octobre 2026

Audit de l'ensemble du dépôt (≈ 175 000 lignes : Swift 69 k, Rust 44 k, TypeScript 26 k, Kotlin 25 k), suivi d'une première passe de remise à niveau. Ce document dit **ce qui a été mesuré, ce qui a été corrigé et ce qui reste**, par ordre de priorité. L'architecture est décrite dans [ARCHITECTURE.md](ARCHITECTURE.md).

## Méthode

- Chaque pile a été compilée, testée et passée à son linter le plus strict avant tout changement, pour avoir une base de comparaison : 430 tests TypeScript, 608 tests Rust, `tsc` strict, clippy (Linux **et** cible `x86_64-pc-windows-gnu` en vérification croisée, pour voir le code `cfg(windows)`).
- Swift (macOS / iOS) ne compile pas sous Linux : cette partie est auditée par lecture et par métriques, sans changement de code hormis un stub de test (voir plus bas).
- La performance a été **mesurée** (Chromium headless, Chrome DevTools Protocol) plutôt que supposée.

## Synthèse

| Zone | État | Priorité de la suite |
| --- | --- | --- |
| TypeScript Windows/Linux | Strict au maximum, lint typé, 0 alerte | Basse : découper les deux plus gros fichiers |
| Rust Windows/Linux | Lints pedantic, verrous uniformes | Moyenne : erreurs typées, `// SAFETY:` |
| Relais Live Activity | Refait, testé, en CI | — |
| Contrat IPC Rust ↔ TS | Événements typés ; commandes encore écrites à la main | **Haute** : générer les types |
| Swift macOS / iOS | Swift 6 + concurrence stricte, mais fichiers géants et singletons | **Haute** : modulariser, tester |
| Kotlin Android | Sain (0 `!!`, 0 `GlobalScope`, 0 `runBlocking`) | Basse |

## Ce qui a été corrigé dans cette passe

| Changement | Pourquoi | Vérification |
| --- | --- | --- |
| **Relais APNs** (`relay/`) : la limite de 4 Kio est appliquée pendant la lecture du corps. | Elle n'était testée que sur `Content-Length` : une requête *chunked* sans cet en-tête était lue en entier avant le contrôle. | Test « corps en flux sans longueur → 413, flux interrompu ». |
| Relais : `unknown` → `LiveActivityRequest` valide par construction ; modules `request` / `payload` / `apns` ; dépendances (`fetch`, horloge) injectées. | Le corps était typé « valide » *avant* d'être validé. | 28 tests vitest, dont la signature ES256 vérifiée par WebCrypto ; `wrangler deploy --dry-run` ; nouveau workflow `relay.yml`. |
| **Bug d'affichage** : une demande de question avec une liste `questions` vide plantait la vue (`q.question` sur `undefined`). | La garde testait `!questions`, or `[]` est vrai. Trouvé par `noUncheckedIndexedAccess`. | Retombe désormais sur « répondez dans le terminal », comme une liste absente. |
| TypeScript : `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitReturns`. | 172 accès indexés supposés définis. | `tsc`, 437 tests, build Vite. |
| ESLint 9 + typescript-eslint `strictTypeChecked` + `stylisticTypeChecked`, en CI (`--max-warnings 0`). | 670 constats ; chaque écart aux presets est commenté dans `eslint.config.js`, aucune désactivation globale. | `npm run lint` à 0. |
| **Événements IPC typés** (`core/events.ts`). | `onEvent<T>` laissait chaque appelant choisir le type du payload (un cast non vérifié) ; 47 écouteurs. | Les noms ont été recoupés avec chaque `emit` du Rust ; un nom mal orthographié ne compile plus. |
| Données d'intégrations lues par `text()` / `errorText()` (`core/decode.ts`). | `String(d.title ?? "")` affichait `[object Object]` si un service renvoyait un objet. | Tests unitaires. |
| Rust : verrous `parking_lot` au lieu de `std::sync`. | ~440 prises de verrou avec trois politiques d'empoisonnement différentes. `parking_lot` était déjà dans l'arbre (rien de nouveau à compiler) ; le hook garde `std` pour rester minimal. | 608 tests Linux, compilation Windows croisée. |
| Rust : politique de lints versionnée (`[workspace.lints]`, `clippy.toml`), `clippy::pedantic` + `unwrap_used`, en CI avec `-D warnings` ; toolchain épinglée (`rust-toolchain.toml`, 1.97.0). | ~470 constats (Linux + Windows). Les exceptions sont listées et justifiées dans `Cargo.toml` ; **0 `unwrap()` hors tests**. Une nouvelle version de Rust (donc de nouveaux lints) ne peut plus casser la CI du jour au lendemain. | `cargo clippy -- -D warnings` à 0 sur Linux et sur `x86_64-pc-windows-gnu` ; 608 tests. |
| **FFI Windows** : `GetTokenInformation` écrit dans un tampon de `u64` au lieu de `u8` (hook et app). | Le code créait une référence `&TOKEN_USER` (alignement 8) sur un `Vec<u8>` (alignement 1) : comportement indéfini si le tampon tombe mal aligné. | Compilation Windows croisée ; `clippy::cast_ptr_alignment` actif, deux exceptions documentées (mémoire allouée par Windows, alignée par l'OS). |
| Lien téléphone : les `unwrap()` sur les en-têtes de trames chiffrées deviennent des erreurs de protocole (`BadHeader`, `BadPadding`). | Ces trames viennent du réseau : un changement de constante futur aurait transformé une trame malformée en panique. | Tests du lien (`phone_link`) verts. |
| Stub de test Swift `IslandView` réaligné (cas `capture`). | Les tests compilent avec des copies des types réels : la copie avait divergé, et `WakeBlockedTests` (qui itère sur `allCases`) ne testait pas cette vue. | Lecture du code testé (`default:` couvre le cas) ; non exécuté ici, faute de toolchain Swift. |

### Performance mesurée

| État de l'île (webview) | Images/s | Thread principal occupé |
| --- | --- | --- |
| Animation d'accueil (au lancement) | ~51 | ~9 % |
| Au repos (replié / rien ne bouge) | **0** | **0,02 %** |

La règle « 0 % de CPU quand l'île est cachée » est tenue côté webview : la boucle de rendu s'arrête d'elle-même. Aucune optimisation n'a été faite là, faute de gain mesurable.

## Ré-audit (après la passe)

Toutes les vérifications ont été relancées de zéro (relais : 28 tests ; TypeScript : `tsc`, lint, 437 tests, build ; Rust : clippy `-D warnings` Linux et Windows, 609 tests ; skills : aucune dérive), puis une revue de code de toute la branche, code environnant compris. Elle a trouvé des **bugs préexistants dans `main`**, vérifiés un par un :

| Constat | Verdict | Suite |
| --- | --- | --- |
| Carte Linear : après création d'un ticket, le champ reste grisé avec l'ancien titre (la carte est reconstruite pendant la requête, le `finally` réactivait l'ancien champ, détaché). | Confirmé (Windows) | **Corrigé** : la fin de requête agit sur le champ affiché. |
| Clé Linear changée ou supprimée : la carte garde les tickets de l'ancienne clé (aucun nouveau sondage, et le cache était vidé *avant* l'écriture de la clé). | Confirmé (Windows) | **Corrigé** : nouveau sondage (ou carte vidée) après l'écriture, comme `keyChanged()` sur Mac. |
| « Mochi regarde une fenêtre » pendant qu'une réponse arrive : la question du regard est perdue et l'ancienne réponse atterrit dans le nouveau chat. | Confirmé (Windows **et** Mac) | **Corrigé sous Windows** : le regard attend la fin de la réponse (2 min max). Mac : même trou dans `WindowLook.look()`, à corriger. |
| Capture de la fenêtre gardée après un échec « pour réessayer », alors que rien ne peut la réutiliser ; « Nouveau chat » ne l'effaçait pas. | Confirmé (Windows) | **Corrigé** : elle vit le temps de sa question, répondue ou non, et « Nouveau chat » l'efface. |
| Le prompt du regard annonce une capture jointe à des modèles qui n'en reçoivent pas (serveurs locaux) : le modèle invente une analyse. | Confirmé (Windows **et** Mac : DeepSeek, Qwen, Moonshot, serveurs locaux) | **Corrigé sous Windows** (test `a_look_tells_a_local_model_that_no_picture_reaches_it`). Mac : à corriger. |
| Mac : un message avec un bloc `image_url` (fournisseur OpenAI/Google) reste dans la conversation, puis est envoyé tel quel à Anthropic si l'on change de fournisseur : erreur 400 sur tous les tours suivants. | Plausible, par lecture (`ClaudeService.swift`) | À vérifier sur un Mac et corriger. |
| Mac : la Mochi du bureau ne suit pas le changement de visage fait dans la garde-robe. | Plausible, par lecture (`DesktopMochi.swift`) | À corriger ; Windows le fait déjà. |
| Mac : chaque capture de « regard » est écrite en JPEG dans `$TMPDIR/CoucouLooks` (les 10 dernières gardées), alors que Windows les garde en mémoire seulement. | Confirmé par lecture | Aligner le Mac sur Windows (mémoire seulement). |
| `BotEngine.look` vaut « portrait » par défaut pour **toutes** les Mochi (iPhone, pilule Flow, récap), alors que son commentaire dit « Main Mochi only ». | Le défaut est un choix produit (commit `45f8a2e`) ; sa portée est ambiguë | **Question au propriétaire du produit.** |
| Couleurs de marque en dur dans `header()` des cartes de services (TS). | Convention existante à toutes les cartes, pas une régression | À migrer vers les couleurs du catalogue de pilules lors d'un passage dédié, sans changer le rendu. |

## Ce qui reste, par priorité

### 1. Contrat IPC Rust ↔ TypeScript généré, pas recopié — *haute*

`core/bridge.ts` (713 lignes) recopie à la main les structures Rust (`Settings`, `PhoneLink*`, …) et 179 commandes. Rien ne détecte une divergence avant l'exécution.
**Proposition** : dériver les types TS des structs Rust avec [`ts-rs`](https://crates.io/crates/ts-rs) (ou `specta`), les écrire dans `src/bindings/`, et faire échouer la CI si le fichier généré diffère. Les événements sont déjà centralisés dans `EventMap`, qui deviendra une simple table de types générés.

### 2. Swift : découper et rendre testable — *haute*

Mesures :

| Constat | Mesure |
| --- | --- |
| Fichiers géants | `IslandViewContent.swift` 5 191 lignes ; `HookServer.swift` 3 833 lignes en **une seule classe** `@unchecked Sendable` ; `SettingsView.swift` 2 496 ; `IslandWindowController.swift` 2 063. |
| Singletons | 1 053 références `.shared`, dont `AppState.shared` ×257 et `KeychainStore.shared` ×104 : couplage implicite, tests impossibles sans l'app. |
| Échappatoires de concurrence | 30 `@unchecked Sendable`, 18 `nonisolated(unsafe)`, 126 `DispatchQueue` dans un projet en concurrence stricte. |
| Observation | 27 `ObservableObject` / 189 `@Published` contre 4 `@Observable`. |
| Réglages | 160 accès `UserDefaults.standard` à clés littérales. |
| Tests | 45 fichiers compilés par `swiftc` via des scripts, avec des **copies** de types (`tests/IslandTypeStubs.swift`) qui divergent sans bruit (cas constaté). |

**Proposition, dans cet ordre :**
1. Sortir la logique pure (file d'approbations, filtres, parseurs, géométrie) dans un paquet SwiftPM local testé avec Swift Testing : les stubs disparaissent, les tests compilent contre les vrais types.
2. Faire de `HookServer` un `actor` (I/O socket) + des types séparés pour le routage des événements et les réponses ; supprimer ses `@unchecked Sendable`.
3. Passer `AppState` et les vues à `@Observable` : invalidation SwiftUI par propriété au lieu de par objet, ce qui compte pour une île toujours affichée.
4. Remplacer les accès directs aux singletons par une injection (environnement SwiftUI / protocoles) dans les nouveaux codes d'abord.
5. Un type `Preferences` avec des clés typées à la place des 160 chaînes `UserDefaults`.

### 3. Rust : erreurs typées — *moyenne*

223 fonctions renvoient `Result<_, String>` et 77 `map_err(|e| e.to_string())`. Le front ne peut pas distinguer « réseau », « clé absente » ou « refusé ».
**Proposition** : une `enum AppError` (`thiserror`, déjà dans l'arbre) sérialisée en `{ kind, message }`, module par module en commençant par les commandes (`secret_*`, `linear_*`, `shell_*`) ; côté TS, `callOrThrow` rejette avec ce type.

### 4. Rust : blocs `unsafe` documentés — *moyenne*

127 blocs `unsafe` (FFI Win32, GTK, libc), 3 commentaires `// SAFETY:`. À documenter un par un — sur une machine Windows, là où ils s'exécutent — puis activer `clippy::undocumented_unsafe_blocks`.

### 5. Formatage — *basse*

Le Rust n'est pas formaté par rustfmt (1 751 blocs diffèrent). Le faire **en un seul commit dédié** (facile à ignorer dans `git blame` via `.git-blame-ignore-revs`), puis ajouter `cargo fmt --check` en CI. Ne pas le mélanger à des changements de fond.

### 6. Logique dupliquée entre plateformes — *basse*

Le moteur de diff, le parseur Markdown, les couleurs de pilules et les raccourcis existent en Swift **et** en TypeScript (et parfois en Kotlin). Le relais Android montre la bonne pratique : `android/relay/test-vectors.json`, généré une fois et vérifié par chaque implémentation. L'étendre à ces portages.

### 7. Gros fichiers TypeScript — *basse*

`settings/main.ts` (1 573 lignes) et `island/island.ts` (1 482) gagneraient à être découpés par section / par responsabilité, sans changer leur rendu (règle : ne pas modifier l'apparence de ce qui est livré).
