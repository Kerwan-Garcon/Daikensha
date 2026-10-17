# Plan : lien Android dans Coucou Mac (phase 0, PROPOSITION, rien n'est construit)

Statut : **en attente de validation.** Aucun code Swift n'est écrit. Ce document répond à la phase 0 : tableau des
messages, liste des fichiers, décision certificat, stratégie de test, et les hypothèses du cahier des charges qui
se sont révélées fausses ou incomplètes.

Lu en entier : `CLAUDE.md`, `docs/ANDROID_LINK.md`, `docs/RELAY_LINK.md`, `android/RELAY_PLAN.md`,
`windows/src-tauri/src/phone_link/{mod,server,admission,pairing,hub}.rs`. Lus en partie (structure, constantes, liste
des tests) : `chat.rs`, `chat_backend.rs`, `discovery.rs`, `relay_client.rs`, `relay_crypto.rs`, `interop.rs`,
`tests.rs`, `RustDesktopInteropTest.kt`, `Pairing.kt` (pinning), `Protocol.kt`. Côté Mac : `project.yml`, les
entitlements, `PhoneLink/*`, `HookServer.swift` (approbations, questions, diffs), `AppState.swift`, `ClaudeService.swift`
(Trousseau, chat), `ClaudePlanGauge.swift`, `CodexPlanGauge.swift`, `CoucouKit/{IslandTypes,MochiWardrobe,DiffEngine,ServiceSnapshot,PillCatalog}.swift`.

---

## 1. Hypothèses du cahier des charges à corriger (je m'arrête et je le dis)

Aucune ne demande de changer le protocole v1 ni l'app Android. Les points marqués **décision** attendent ton choix (section 8).

1. **Entitlement « serveur réseau » : inutile.** La cible GitHub `NotchBuddy` n'est pas sandboxée
   (`ENABLE_SANDBOX: NO`, aucun `com.apple.security.app-sandbox` dans `Coucou.entitlements`,
   `CoucouCloud.entitlements` ni `CoucouReleaseCloud.entitlements`). `com.apple.security.network.server` est un
   entitlement de sandbox : sans sandbox il n'a aucun effet, et le hardened runtime ne filtre pas le réseau. Je propose
   de **ne pas l'ajouter** (l'ajouter ne casse rien mais laisse croire qu'il protège quelque chose).
2. **`SwiftDesktopInteropTest` impose de toucher `android/`.** `RustDesktopInteropTest.kt` lance `cargo` en dur : on ne
   peut pas le faire pointer vers un serveur Swift sans ajouter un fichier Kotlin. Ce serait un fichier **de test
   seulement** (`android/app/src/test/.../SwiftDesktopInteropTest.kt`), l'app ne change pas d'un octet, mais c'est
   contraire à « ne modifie pas `android/` ». **Décision D4.**
3. **Je ne peux ni compiler ni lancer de Swift ici.** Cette session tourne dans un conteneur Linux : pas de Xcode, pas
   de `swiftc`, pas de Network.framework, Security ni CryptoKit. Les étapes « `xcodebuild` doit passer » et tous les
   tests Swift tournent donc sur ton Mac ou sur le runner macOS de `.github/workflows/build.yml`, qui ne se déclenche
   que sur `push` vers `main` et sur les PR vers `main`. **Décision D5.**
4. **Branche.** Cette session doit pousser sur `claude/android-link-mac-x2fhwc`, pas sur `feat/mac-android-link`. Je
   travaille donc sur celle-là ; tu pourras la renommer au moment de la PR.
5. **`URLSessionWebSocketTask` : je recommande plutôt `NWProtocolWebSocket`.** Le relais demande un en-tête
   `Authorization: Bearer …` et deux sous-protocoles. Apple classe `Authorization` parmi les en-têtes « réservés » de
   `URLRequest`. En pratique ça passe le plus souvent, mais rien ne le garantit. `NWProtocolWebSocket.Options` a
   `setAdditionalHeaders` et `setSubprotocols`, reste dans Network.framework comme le serveur, et me laisse contrôler
   les trames ping/pong et les codes de fermeture (`1000 replaced`, `1008 …`) dont `relay_client.rs` a besoin. Toujours
   zéro dépendance.
6. **`SecIdentity` sans Trousseau.** Pour donner à `NWProtocolTLS` une `sec_identity` à partir d'une clé et d'un
   certificat en mémoire, macOS n'a pas d'API publique fiable (`SecIdentityCreate` est privée, et je ne suis pas sûr
   que `kSecImportToMemoryOnly` existe sur macOS 15). La voie sûre : la clé privée (`kSecClassKey`) et le certificat
   (`kSecClassCertificate`) vont dans le Trousseau, et on demande `kSecClassIdentity`. C'est exactement « clé privée
   uniquement dans le Trousseau ». Détails en section 5.
7. **Empreinte des approbations.** `ApprovalRelay.fingerprint` (iPhone) hache `inputKey` = le `tool_input` en JSON. Pour
   Android je propose de faire comme Windows : `inputKey` = l'identifiant de la requête (`ApprovalInfo.id.uuidString`),
   et la commande hachée est celle **coupée à 500 caractères** (celle qui est envoyée). Le téléphone traite l'empreinte
   comme une valeur opaque qu'il renvoie, donc ce n'est pas un changement de protocole. Bénéfice : deux demandes
   identiques ont deux empreintes différentes, et une décision tardive ne peut jamais répondre à la suivante. Le lien
   iPhone garde sa propre dérivation, sans changement.
8. **Les questions Mac n'ont pas d'identifiant de requête.** `AskQuestion` ne contient que les questions. `HookServer`
   garde `questionSessionId` et le descripteur du hook, mais rien de stable à mettre dans l'empreinte, et pas le
   `pillId`. Il faut un petit ajout dans `HookServer` (un `UUID` et le `pillId` de la question en attente, posés dans
   `processQuestionRequest`). Le comportement de l'île ne change pas.
9. **Chat : pas d'outils, et les fournisseurs du Mac.** Le chat de l'île appelle Anthropic avec l'outil
   `web_search` : le chemin téléphone n'enverra **aucun** `tools`. Les fournisseurs du Mac (`ChatProvider` :
   anthropic, google, openai, ollama, lmstudio, deepseek, qwen, moonshot) ne sont pas ceux de Windows (openrouter,
   custom). Le téléphone accepte n'importe quel `provider` (texte libre dans `Protocol.kt`), donc le Mac propose les
   siens. `ClaudeService.chat` écrit directement dans `AppState` (l'île) : il faut ajouter une entrée « téléphone »
   séparée, sans toucher au chemin de l'île.
10. **Les cartes de services iPhone en disent trop.** `ServiceSnapshot` (iPhone) contient montants, titres, e-mails et
    liens, chiffrés par iCloud. Le tableau « Never sent » d'`ANDROID_LINK.md` est plus strict. Il faut donc un
    adaptateur Android à part, construit depuis les mêmes données d'`AppState`, et non une réutilisation du snapshot.
11. **Confidentialité du réseau local (macOS 15).** L'annonce Bonjour et le trafic LAN déclenchent l'invite système
    la première fois. `NSLocalNetworkUsageDescription` et `NSBonjourServices` (`_coucou._tcp`) vont dans les
    `info.properties` de la cible `NotchBuddy` (elles s'appliquent à toutes ses configurations, le code du lien aussi),
    jamais dans `CoucouAppStore`.
12. **`SessionPublisher`, `ApprovalRelay`, `QuestionRelay`, `ServicePublisher` sont des transports CloudKit**
    (`#if PHONE_LINK`), pas des sources de données. Les vraies sources sont `AppState` (`tasks`, `pendingApproval`,
    `pendingQuestion`, `sessionDiffs`, `claudePlanUsage`, `codexPlanUsage`, `mochiOutfitSelection`, les données des
    pollers) et `HookServer` (`sendApprovalDecision(_:for:)`, `sendQuestionAnswers(_:)`). Le lien Android s'y branche
    directement, comme ces classes, et reprend leurs gardes (pas de carte démo, `hasRealPendingQuestion`). Il ne
    dépend pas de `PHONE_LINK`, donc les deux liens restent indépendants.

---

## 2. Message ou capacité → source Mac existante → ce qui manque

| Message / capacité | Source Mac existante | Ce qui manque |
|---|---|---|
| `hello` / `welcome` / `error` | — | Tout : `Conversation.swift` (port de `read_loop`), comparaison en temps constant, pause de 400 ms sur un mauvais jeton, `os: "macos"` |
| `ping` / `pong` / `bye` | — | Dans `Conversation.swift` |
| Ligne > 64 KiB | — | `LineReader.swift` (pur, ne lit jamais plus d'un octet au-delà de la limite) |
| Admission (8 / 2 / 4 / 8 adresses, 10 s ou 3 s) | — | `Admission.swift`, port ligne à ligne d'`admission.rs` |
| Filtre réseau local | — | `isLocal(_:)` (privé, loopback, link-local, v4 mappée en v6) |
| `sessions` (champs v1) | `AppState.tasks` (`AgentTask` : `id`, `name`, `state: BotState`, `steps`, `stepIndex`), filtré par `PillCatalog.isSession` comme `SessionSnapshot.all` | Adaptateur → `SessionIn` (`agent` = nom, `statusText` = dernière étape ou nom de fichier du dernier diff comme Windows, `stepCount` = nombre d'étapes). Les 11 états de `BotState` sont exactement ceux du protocole. Les IDs de pills de session sont les mêmes que sur Android (vérifié : seuls `integration_crypto/mail/music/portfolio` sont propres au Mac, et ce ne sont pas des sessions) |
| `approval` / `decision` / `approvalResolved` | `AppState.pendingApproval: ApprovalInfo` (tête de la file `ApprovalQueue`) ; `HookServer.sendApprovalDecision(_:for: id)` refuse déjà un id qui n'est plus en tête | Le hub (empreinte, TTL de 115 s, une seule décision, `truncated` → `allow` refusé) ; ignorer `demo_session` ; jamais `always` |
| cap `answers` : `question` / `answer` | `AppState.pendingQuestion: AskQuestion` ; `HookServer.sendQuestionAnswers(_:)` ; `hasRealPendingQuestion` | Identifiant de requête et `pillId` de la question (point 1.8) ; vérification sur le main actor que la question est toujours la même avant d'appliquer |
| cap `details` | `AgentTask.steps`, `finalLine`, `sessionCwd` (→ nom du dossier seulement), `color` | Nettoyage (20 étapes, 200 caractères, budget de 56 KiB) dans le hub |
| cap `diffs` : liste + `getDiff` / `diff` | `AppState.sessionDiffs[pillId]: [FileDiff]` (`id`, `path`, `added`, `removed`, `hunks`, `tooLarge`, `isNewFile`) | Conversion hunks → `[kind, text]` ; réponse synchrone sur le main actor (pas d'aller-retour webview comme sur Windows) |
| cap `usage` | `AppState.claudePlanUsage: PlanUsage`, `AppState.codexPlanUsage: CodexPlanUsage` (`resetCredits`, `planType`) | Adaptateur → `UsageIn`. Le Mac ne connaît pas le nom du plan Claude : `plan` est omis côté Claude (le champ est facultatif) |
| cap `services` | Données des pollers dans `AppState` (`stripePayments`, `stripeBalance`, `githubStats`, `vercelDeployments`, `resendEmails`, `resendTotal`, `notionPages`, `calcomBookings`, n8n) | Adaptateur réduit qui suit le tableau « Never sent », une case par service (point 1.10) |
| cap `prefs` | `AppState.mochiOutfitSelection: Outfit` : les 13 valeurs brutes sont identiques à `OUTFIT_CHOICES` | Abonnement + test qui compare la liste |
| cap `chat` | `ClaudeService` (Anthropic), `chatOpenAICompatible` (Google, OpenAI, DeepSeek, Qwen, Moonshot), `LocalChat.streamChat` (Ollama, LM Studio, en streaming), clés via `KeychainStore` | `ChatLink.swift` (port des règles de `chat.rs`) + `PhoneChatBackend.swift` : conversation à part en mémoire, sans contexte, sans outils, erreurs classées par statut HTTP et jamais par leur texte ; liste fournisseur/modèle cochée dans les réglages |
| cap `relay` : `relayAccess` | — | `RelayCrypto.swift`, `RelayClient.swift`, champs `relay/room/key/access` du lien |
| Annonce Bonjour | — | `Discovery.swift` (`NWListener.service`, TXT `v=1`, `fp=<16 hex>`) |
| Appairage / QR | — | `Pairing.swift` (jeton, lien, encodage `%`, adresse LAN, nom), QR via `CIQRCodeGenerator` |
| Identité TLS | `Keychain` (`ClaudeService.swift`, mots de passe génériques, erreurs ignorées) | `LinkKeychain.swift` (erreurs remontées, clés et certificat), `Certificate.swift` |
| Réglages | `GroupBox("iPhone")` de `SettingsView` (style des interrupteurs) | `AndroidLinkSettingsView.swift`, inséré à côté sous `#if ANDROID_LINK` |

---

## 3. Fichiers

### Nouveaux, dans `NotchBuddy/Sources/App/AndroidLink/`, tous entièrement sous `#if ANDROID_LINK`

Les fichiers purs (sans socket, sans horloge, sans `AppState`) se compilent seuls avec `swiftc` pour les tests, comme
`ApprovalQueue.swift` aujourd'hui.

| Fichier | Rôle | Équivalent Rust | Pur ? |
|---|---|---|---|
| `LinkProtocol.swift` | constantes (port 47821, 64 KiB, 115 s…), `clip`, `constantTimeEqual`, `fingerprint`, `hex`, `isLocal`, `isToken` | morceaux de `hub.rs`, `server.rs`, `pairing.rs` | oui |
| `LineReader.swift` | découpe en lignes, limite de 64 KiB, `\r\n` | `LineReader` | oui |
| `Admission.swift` | admission et tickets (libérés à la fin de la connexion, quelle qu'elle soit) | `admission.rs` | oui |
| `LinkHub.swift` | sessions, approbation, question, services, usage, tenue, diffs ; abonnés ; règles de décision et de réponse | `hub.rs` | oui (verrou `Mutex` de `Synchronization`, une file de sortie bornée par abonné) |
| `ChatLink.swift` | règles du chat (id, 4000 caractères, `busy`, 12 envois en 10 min, morceaux de 8 KiB, annulation) | `chat.rs` | oui |
| `Certificate.swift` | DER X.509 minimal + signature CryptoKit P-256 (option A) | `rcgen` dans `pairing.rs` | oui (CryptoKit seulement) |
| `RelayCrypto.swift` | HKDF, HMAC, AES-256-GCM, trames, compteurs, remplissage à 128 | `relay_crypto.rs` | oui (CryptoKit seulement) |
| `Discovery.swift` | contenu de l'annonce (pur) + publication via `NWListener.service` | `discovery.rs` | contenu oui |
| `Pairing.swift` | jeton, lien, encodage `%`, nom de l'ordinateur, adresse LAN, image QR | `pairing.rs` | lien et jeton oui |
| `LinkKeychain.swift` | `SecretStore` (protocole + Trousseau), identité TLS | `pairing::SecretStore`, `secrets.rs` | non |
| `Conversation.swift` | une conversation v1 sur un transport quelconque (TLS ou relais) : hello, capacités, boucle, inactivité | `run_with`, `read_loop` | non (mais testable sur loopback) |
| `LinkServer.swift` | `NWListener` + `NWProtocolTLS` (TLS 1.2 à 1.3), filtre, admission, délais | `serve`, `one_connection`, `tls_acceptor` | non |
| `RelayClient.swift` | WebSocket sortant (`NWProtocolWebSocket`), handshake `init`/`accept`, battement de 25 s, silence de 80 s, reprises | `relay_client.rs` | non |
| `PhoneChatBackend.swift` | vrai fournisseur pour le chat du téléphone | `chat_backend.rs` | non |
| `LinkFeed.swift` | `AppState` → hub (Combine, regroupé à 300 ms) ; décisions et réponses → `HookServer` | rôle de `island/phone-link.ts` + `TauriHost` | non |
| `AndroidLinkController.swift` | `@MainActor` : démarrage, arrêt, réglages, « Appairer à nouveau », état affiché | `mod.rs` | non |
| `AndroidLinkSettingsView.swift` | section « Android phone » des réglages | `Settings → Android phone` | non |

### Fichiers existants touchés (ajouts seulement, aucun changement d'apparence)

- `NotchBuddy/project.yml` : `ANDROID_LINK` ajouté à `SWIFT_ACTIVE_COMPILATION_CONDITIONS` des quatre configurations
  de `NotchBuddy` (`Debug` en a besoin pour que la commande de build de `CLAUDE.md` compile le code), **jamais** à
  `CoucouAppStore` ; `NSLocalNetworkUsageDescription` et `NSBonjourServices` dans `info.properties` de `NotchBuddy`.
  `Resources/Info.plist` est régénéré et commité.
- `HookServer.swift` : identifiant et `pillId` de la question en attente (point 1.8).
- `ClaudeService.swift`, `LocalChat.swift` : une entrée « une réponse pour le téléphone » (sans `AppState`, sans
  outils, sans contexte), à côté des fonctions de l'île, qui restent identiques.
- `SettingsView.swift` : une ligne `#if ANDROID_LINK AndroidLinkSettingsView() #endif` après la `GroupBox("iPhone")`.
- `AppDelegate.swift` : démarrage si activé, arrêt (retrait de l'annonce) à la fermeture.
- `Resources/Localizable.xcstrings` : les nouvelles chaînes (voir D7).
- `docs/ANDROID_LINK.md` : « Status » et nouvelle section « macOS implementation notes ».
- `scripts/test-android-link*.sh`, `tests/AndroidLink*Tests.swift`, et peut-être `.github/workflows/build.yml` (D5).

### Ce qui reste partagé avec le lien iPhone

Les sources de données (`AppState`, `HookServer`, `DiffEngine`, `PillCatalog`, `Outfit`, les jauges) et les gardes
démo. Rien de CloudKit. Les deux liens peuvent être allumés en même temps : une décision appliquée d'un côté ferme la
carte, l'autre côté voit `pendingApproval` changer et retire sa demande (`approvalResolved` côté Android). Le code
iPhone n'est pas modifié.

---

## 4. Certificat auto-signé : option A ou B

**Option A, DER à la main (~150 lignes).** Un encodeur ASN.1 DER minimal (SEQUENCE, INTEGER, OID, BIT STRING,
UTF8String, UTCTime / GeneralizedTime, `[0]`) et un `TBSCertificate` v3 fixe : numéro de série aléatoire positif de
16 octets, `ecdsa-with-SHA256` (1.2.840.10045.4.3.2), émetteur = sujet = `CN=Coucou phone link`, validité
2024-01-01 → 2124-01-01 (UTCTime avant 2050, GeneralizedTime après, comme le veut la RFC 5280), clé publique
`id-ecPublicKey` + `prime256v1` avec les 65 octets de `x963Representation`. Signature :
`P256.Signing.PrivateKey.signature(for: tbs).derRepresentation`. Mêmes champs que ce que `rcgen` produit dans
`pairing.rs`.
- Pour : zéro dépendance, respecte la règle à la lettre ; code fait une fois, sans état, entièrement testable.
- Risque : un DER faux = un certificat refusé à la poignée de main. Il est couvert par les tests :
  `SecCertificateCreateWithData` doit l'accepter, `SecCertificateCopyKey` doit rendre la même clé publique,
  `SecKeyVerifySignature(.ecdsaSignatureMessageX962SHA256)` doit valider la signature,
  `openssl x509 -text -noout` doit le lire (script de test), et surtout une vraie poignée de main TLS sur loopback,
  puis le client Android (Conscrypt) en interop. Le téléphone ne vérifie que le SHA-256 de la feuille (`Pairing.kt`),
  il faut seulement que le certificat se lise et que sa clé signe le `CertificateVerify`.

**Option B, `apple/swift-certificates`.** Écrit par Apple, solide, API agréable.
- Contre : c'est un paquet SwiftPM hors SDK, qui en entraîne trois autres (`swift-asn1`, `swift-crypto`, et selon la
  version `swift-collections`). Ça fait quatre dépendances pour un appel exécuté une seule fois dans la vie de
  l'installation. La règle dit « sauf si vraiment inévitable » : ici c'est évitable. (FluidAudio existe déjà, mais lui
  apporte un modèle de reconnaissance vocale impossible à réécrire.)

**Mon avis : option A.** Le risque est petit, connu, et entièrement couvert par des tests qui tournent à chaque
build ; une dépendance, elle, reste pour toujours. Si l'option A échoue à l'interop d'une façon que je ne sais pas
corriger rapidement, je reviens vers toi avant de passer à B. **Décision D1.**

---

## 5. Trousseau et identité TLS

- **Éléments** (service `fr.louisraille.NotchBuddy`, mêmes noms que sur Windows quand ça s'applique) :
  clé privée P-256 (`kSecClassKey`, permanente, libellé `Coucou phone link`), certificat (`kSecClassCertificate`, même
  libellé) → `SecIdentity` demandée par `kSecClassIdentity`. Jeton `phone-link-token`, et pour le relais
  `phone-relay-room`, `phone-relay-key`, `phone-relay-access` (mots de passe génériques, non synchronisés).
- **Ordre** : la clé d'abord, puis le certificat (comme `pairing.rs`). Une clé illisible ou une identité introuvable
  → nouvelle identité (les téléphones doivent alors s'appairer à nouveau, comme sur Windows).
- **Échec = le lien ne démarre pas.** Contrairement au `Keychain.save` actuel qui ignore les erreurs, chaque
  écriture vérifie son `OSStatus`. Aucun repli sur disque, et l'erreur s'affiche dans les réglages.
- **Quel Trousseau.** Je propose le Trousseau « data protection » (`kSecUseDataProtectionKeychain`,
  `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`) : il est lié à l'identifiant d'application signé, sans invite
  « Coucou veut utiliser la clé… » à chaque nouvelle build. Il exige une app signée : ça marche avec
  `scripts/dev-install.sh` (DebugCloud) et avec Release ; dans une build `Debug` non signée, le lien affichera
  « Trousseau indisponible » et ne démarrera pas. Variante : le Trousseau « login » classique, comme les autres
  secrets de Coucou, qui marche aussi sans signature, mais au prix d'invites d'autorisation à chaque build. **Décision D2.**
- Le jeton est généré avec `SecRandomCopyBytes` (32 octets → 32 caractères `[A-Za-z0-9_-]`, comme `random_token`).
  Les clés relais (K, salle) viennent du même générateur.

---

## 6. Comportement, concurrence, performance

- **Off = rien.** Tant que l'interrupteur est éteint : pas de `NWListener`, pas d'annonce, pas d'abonnement Combine,
  pas de minuterie. On l'éteint : on dit `error closed` aux téléphones, on annule l'écoute, on retire l'annonce, on
  coupe le relais et on oublie la conversation de chat.
- **On, au repos : 0 % de CPU.** Aucune minuterie périodique pour le LAN. Le délai du `hello` et l'inactivité de
  20 min sont des minuteries ponctuelles par connexion, réarmées à chaque ligne reçue. Le flux `AppState` → hub suit
  les changements (Combine), sans sondage. Seul le relais, quand il est allumé, garde un battement de 25 s (imposé
  par `RELAY_LINK.md`, comme sur Windows).
- **Concurrence (Swift 6 strict).** Réseau sur une `DispatchQueue` série dédiée. Hub et admission : classes
  `Sendable` protégées par un `Mutex` (Synchronization, macOS 15). `LinkFeed` et les appels à `HookServer` sur le
  main actor. Une décision du téléphone saute sur le main actor et **vérifie à nouveau** que la tête de la file est la
  même requête avant d'appeler `sendApprovalDecision(_:for:)`, comme `ApprovalRelay.apply`.
- **Ne jamais bloquer Claude Code.** Le lien ne touche jamais au chemin du hook : il ne fait que lire `AppState` et
  appeler les mêmes fonctions qu'un clic. La carte du bureau reste utilisable tout du long.
- **Écoute.** IPv4 `0.0.0.0:47821`, sinon un port libre (`EADDRINUSE` arrive dans `stateUpdateHandler`, puis on
  réessaie avec `.any`). Tout pair qui n'est pas local est annulé avant la poignée de main TLS.
- **Journal.** Uniquement des états (« listening on port N », « phone connected », « decision allow »), jamais un
  jeton, une clé, une salle, une adresse ni le texte d'une commande. Un test lit les sources pour vérifier qu'aucune
  interpolation de secret n'arrive dans un log (comme `the_source_never_logs_or_prints`).

---

## 7. Tests (phase 4, préparés dès la phase 1)

**Forme.** Le dépôt n'a pas de cible XCTest : ses tests sont des exécutables `swiftc` (`tests/*.swift` lancés par
`scripts/test-*.sh`, et appelés par `build.yml`). Je suis la même forme :

- `scripts/test-android-link.sh` : les fichiers purs + `tests/AndroidLinkTests.swift`. Il couvre l'admission (les
  10 tests d'`admission.rs`), le hub (les 52 tests de `hub.rs` : décision une fois, mauvaise empreinte, requête
  remplacée, répondu au bureau, décision tardive, limite exacte de 115 s, `truncated`, détails, budget de 56 KiB,
  fichiers, diffs en parties, limite de demandes, usage, services, tenue, questions), le `LineReader`, `isLocal`, le
  jeton et le lien (y compris le test des 2 048 caractères), l'annonce (aucun secret dedans), les règles du chat avec
  un faux fournisseur, et le certificat (lu par Security, signature valide, empreinte = SHA-256 du DER).
- `scripts/test-android-link-server.sh` : le vrai serveur `NWListener` sur loopback, avec un client de test
  `NWConnection` + TLS qui épingle le certificat comme le téléphone. Ce sont les miroirs des 65 tests de `tests.rs` : mauvais
  jeton, jeton d'une autre longueur, pas de `hello` d'abord, autre empreinte, décision tardive, répondu au bureau,
  ligne de 64 KiB + 1 (et exactement 64 KiB acceptée), autre certificat attendu, texte en clair refusé, réappairage,
  extinction, limite totale, deux connexions muettes par adresse, inondation depuis une ou plusieurs adresses (via les
  alias `127.0.0.x`, comme `loopback_aliases`), commande de 10 000 caractères, plus toutes les sections de capacités
  et le relais de bout en bout contre un faux relais local en `ws://`.
- `scripts/test-android-link-relay.sh` : `RelayCrypto` contre `android/relay/test-vectors.json` **octet pour octet**
  (`mustRefuse`, `mustRefuseInOrder`, `data`, clés dérivées, poignée de main), plus la propriété « aucune paire (clé,
  nonce) répétée », le refus à 2^32 − 1, et HKDF contre la RFC 5869 A.1.
- **Interop.** `tests/AndroidLinkInteropServer.swift` : le même programme que `interop.rs`, même protocole sur
  stdin/stdout (`{"listening"…}`, `OK <commande>`, `DECISION allow|deny <id>`, `GETDIFF …`). Ensuite, au choix (D4) :
  (a) un `SwiftDesktopInteropTest.kt` de test seulement, copie de `RustDesktopInteropTest` qui lance ce programme
  quand `COUCOU_SWIFT_INTEROP=1` (lancé sur ton Mac : `./gradlew test` dans `android/`) ; ou (b) sans toucher
  `android/`, un script Node `scripts/android-link-replay.mjs` qui rejoue la séquence du client Kotlin (TLS épinglé,
  hello avec toutes les capacités, approbation, décision, question, diff, chat, relais avec les vecteurs). (a) teste
  le vrai client, (b) une copie de son comportement.
- **Builds.** `xcodebuild -scheme NotchBuddy -configuration Debug build`, `-configuration Release`, et
  `-scheme CoucouAppStore` qui doit compiler sans aucun symbole du lien (vérifié par `nm` : aucun `AndroidLink`).
- **Test manuel documenté** dans `docs/ANDROID_LINK.md` : appairage par QR, approbation à l'écran verrouillé,
  réponse à une question, chat, passage en relais hors Wi-Fi, « Appairer à nouveau », extinction.

---

## 8. Décisions à prendre

| # | Question | Ma recommandation |
|---|---|---|
| D1 | Certificat : A (DER à la main) ou B (`swift-certificates`) | **A** |
| D2 | Trousseau « data protection » (app signée obligatoire) ou Trousseau « login » | **data protection** |
| D3 | WebSocket du relais : `NWProtocolWebSocket` au lieu de `URLSessionWebSocketTask` | **oui** |
| D4 | Interop : (a) ajouter un fichier de test Kotlin dans `android/app/src/test`, ou (b) un script de rejeu sans toucher `android/` | **(a)**, l'app Android ne change pas |
| D5 | Compilation pendant le travail : (a) tu lances `xcodebuild` et les scripts sur ton Mac à chaque fin de phase ; (b) j'ouvre une PR brouillon vers `main` pour que `build.yml` (macOS) compile à chaque push ; (c) j'ajoute la branche aux déclencheurs de `build.yml` | **(b)**, plus (a) pour le test manuel. J'ajouterais aussi les nouveaux scripts de test à `build.yml` |
| D6 | Empreinte Android des approbations = identifiant de requête (comme Windows) plutôt que `tool_input` (comme l'iPhone) | **identifiant de requête** |
| D7 | Langues des nouvelles chaînes : anglais seulement, ou anglais + français (les 8 autres langues retombent sur l'anglais) | **anglais + français** |
| D8 | Pas d'entitlement `network.server` (cible non sandboxée) | **ne pas l'ajouter** |

## 9. Découpage en commits (après validation)

Phase 1 : `project.yml` + Info.plist → fichiers purs (protocole, lignes, admission) et leurs tests → certificat + Trousseau
→ hub (sessions, approbation) → serveur + conversation → appairage, QR, Bonjour → contrôleur + flux `AppState` →
réglages → tests serveur. Phase 2 : une capacité par commit, dans l'ordre `answers`, `details`, `diffs`, `usage`,
`services`, `prefs`, `chat`. Phase 3 : crypto relais + vecteurs → client relais → champs du lien, rotation, réglages.
Phase 4 : interop, `build.yml`, notes macOS dans `docs/ANDROID_LINK.md`. À la fin de chaque phase je te montre le diff
et j'attends ton accord.
