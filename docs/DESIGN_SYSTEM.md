# Design system de l'app Windows et Linux

Toutes les couleurs et les polices de `windows/` sont décidées dans **un seul fichier** : `windows/src/design/tokens.css`. Les feuilles de style et le code TypeScript ne font que s'y référer. Changer une valeur là change toute l'app d'un coup.

Pas de librairie : ce sont des variables CSS natives (`--color-accent`), lues aussi par le TypeScript. Rien à installer, rien à générer, et le fichier se modifie à la main.

## Où sont les choses

```
windows/
├── index.html, settings.html, pill.html, circle.html, mochi.html
│     Une page par fenêtre : la structure et les <link> vers ses styles, rien d'autre.
└── src/
    ├── design/
    │   ├── tokens.css   ← LE fichier à modifier : polices, couleurs
    │   └── tokens.ts    ← les mêmes valeurs côté TypeScript (lues dans tokens.css)
    ├── styles/          ← tout le style, une feuille par fenêtre
    │   ├── island.css   ← l'island : liste ordonnée de ses parties
    │   ├── island/      ← une feuille par partie (cards.css, chat.css, shell.css…)
    │   ├── settings.css, hub.css   ← la fenêtre Coucou (réglages)
    │   ├── pill.css     ← la pill de la dictée
    │   ├── circle.css   ← l'overlay « Entoure et demande »
    │   └── mochi.css    ← Mochi sur le bureau
    └── views/, island/, settings/…   ← le comportement (TypeScript), pas de couleurs en dur
```

Chaque page charge `tokens.css` en premier, puis sa feuille :

```html
<link rel="stylesheet" href="/src/design/tokens.css" />
<link rel="stylesheet" href="/src/styles/island.css" />
```

## Les trois couches de `tokens.css`

1. **Polices** : `--font-sans`, `--font-mono`.
2. **Rôles** (`--color-…`) : à quoi sert une couleur. C'est ici qu'on restyle.
   - Textes : `--color-text` (le plus clair) → `--color-text-muted` → `--color-text-subtle` → `--color-text-faint` → `--color-text-disabled` → `--color-text-ghost`.
   - Fonds : `--color-bg`, `--color-surface-sunken`, `--color-surface`, `--color-surface-raised`, `--color-surface-selected`, `--color-field`.
   - Calques transparents : `--color-overlay-rgb` (blanc, pour les remplissages, filets et survols) et `--color-scrim-rgb` (noir, pour les ombres).
   - Accents : `--color-accent` (bleu de l'island et du Shell) et ses liens `--color-link`, `--color-brand` (orange de la fenêtre Coucou).
   - États : `--color-danger`, `--color-success`, `--color-warning`, etc.
3. **Palette** : les autres nuances exactes déjà utilisées, nommées par teinte et luminosité. `--gray-718` est un gris à 71,8 % de luminosité, `--amber-57` un ambre à 57 %.

### Couleurs transparentes

Une couleur qui existe aussi en transparent est donnée en **canaux R G B** :

```css
--color-accent-rgb: 96 165 250;
--color-accent: rgb(var(--color-accent-rgb));
```

et s'utilise ainsi dans les feuilles :

```css
background: rgb(var(--color-accent-rgb) / 0.22);  /* l'accent à 22 % */
```

Changer `--color-accent-rgb` change donc la couleur pleine **et** toutes ses versions transparentes.

## Recettes

**Changer la couleur d'accent de l'island** : dans `tokens.css`, remplacer `--color-accent-rgb: 96 165 250;` par d'autres canaux (par exemple `244 114 182` pour du rose). Le Shell, les sélections, les interrupteurs et les anneaux suivent. Les liens ont leurs propres teintes plus claires de l'accent (`--color-link`, `--color-link-hover`), juste en dessous : les changer en même temps.

**Changer la police** : modifier `--font-sans` (ou `--font-mono` pour les chiffres et le code).

**Éclaircir ou assombrir les fonds** : modifier les `--color-surface-…` et `--color-bg`.

**Rendre les filets plus visibles** : `--color-line` et `--color-hairline`, ou l'opacité directement dans la feuille concernée.

**Changer le style d'un composant** : ouvrir sa feuille dans `src/styles/island/` (le nom suit la partie : `chat.css`, `cards.css`, `shell.css`…) et n'y utiliser que des tokens. Si une couleur manque, l'ajouter dans `tokens.css` plutôt que de l'écrire en dur.

**Ajouter une partie à l'island** : créer `src/styles/island/ma-partie.css`, puis l'ajouter dans `src/styles/island.css` **à sa place**. L'ordre de cette liste est l'ordre de la cascade : un fichier placé plus bas l'emporte à spécificité égale.

**Ajouter une fenêtre** : une page HTML qui lie `tokens.css` puis sa feuille dans `src/styles/`, et l'ajouter aux entrées de `vite.config.ts`.

**Faire un thème clair** : ajouter un second bloc qui redéfinit la couche 2, par exemple :

```css
:root[data-theme="light"] {
  --color-overlay-rgb: 0 0 0;
  --color-scrim-rgb: 0 0 0;
  --color-text: #15171b;
  --color-text-muted: #555b65;
  --color-bg: #f4f5f7;
  --color-surface: #ffffff;
  /* … les autres rôles */
}
```

puis poser `data-theme="light"` sur `<html>`. Les valeurs claires des maquettes sont dans `design/mockups/windows-shell/Controles.dc.html` (palette `light`).

## Depuis le TypeScript

`src/design/tokens.ts` lit `tokens.css` au moment du build. Il n'y a donc pas de deuxième copie des valeurs.

```ts
import { cssVar, colorHex, colorAlpha } from "../design/tokens";

el.style.color = cssVar("--color-text-subtle");      // "var(--color-text-subtle)"
ctx.strokeStyle = colorHex("--color-accent");        // "#60a5fa", pour un canvas
row.style.background = `${colorHex("--color-success")}14`; // suffixe d'alpha hexa
ctx.fillStyle = colorAlpha("--color-accent-rgb", 0.1);     // "rgba(96, 165, 250, 0.1)"
```

- `cssVar` pour un style posé depuis le code : il suit la feuille, et un futur thème avec.
- `colorHex` ou `colorAlpha` quand il faut une vraie couleur : canvas, calcul, `hexToRGB()`.
- Un nom inconnu lève une erreur dès qu'il est utilisé.

Restent volontairement en dur dans le code : les **couleurs de marque** (agents, intégrations, fournisseurs d'IA, vert Spotify, verts des contributions GitHub, couleurs des pills, qui sont des données partagées avec le Mac) et les **dessins** (Mochi et ses tenues, l'image partagée du récap, le canvas de dépôt de fichiers).

## Ce qui n'est pas encore en tokens

Les **tailles, espacements et rayons** restent écrits dans chaque feuille (`padding: 16px`, `border-radius: 20px`). Les regrouper changerait des pixels dans des vues déjà livrées, ce que `CLAUDE.md` interdit sans demande. Si un jour c'est voulu, on pourra ajouter une échelle (`--space-…`, `--radius-…`) de la même façon que pour les couleurs.

## Garde-fous

`npm test` lance `tests/design-tokens.test.mjs`, qui échoue si :

- une feuille de `src/styles/` écrit une couleur elle-même (`#…`, `rgb(12…)`) au lieu d'un token ;
- une `var(--…)` pointe vers une variable qui n'existe pas ;
- le code demande un token inconnu (`colorHex("--color-acent")`) ;
- une page HTML contient du style (`<style>`, `style="…"`) ;
- une feuille est importée depuis le TypeScript au lieu d'être liée par sa page.

## Comment ça a été migré

Le passage à ce système (octobre 2026) a remplacé chaque valeur par un token de **la même valeur** : rien n'a bougé à l'écran. Ça a été vérifié en comparant le style calculé des 5 000 éléments de toutes les fenêtres avant et après la migration.

Seule différence : les fenêtres réglages, pill et overlay utilisent maintenant la même pile de polices que l'island. Pour du texte latin, le rendu est identique (`system-ui` reste en tête).
