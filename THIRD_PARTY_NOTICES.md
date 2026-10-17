# Notices des composants tiers

Le code Rust du workspace est sous licence MIT, voir [`LICENSE`](LICENSE).
Les ressources regroupées dans `assets/` peuvent avoir des licences
différentes.

## Polices

| Fichiers | Provenance | Licence |
| --- | --- | --- |
| `FreeSans*.ttf` | GNU FreeFont | GNU GPL v3 ou ultérieure, avec les conditions de distribution de la fonte ; [projet GNU FreeFont](https://savannah.gnu.org/projects/freefont/) |
| `MaterialIcons-Regular.ttf` | Google Material Icons | Apache License 2.0 ; [dépôt Material Icons](https://github.com/google/material-design-icons) |
| `fa-solid-900.ttf` | Font Awesome 6 Free | Police sous SIL OFL 1.1 ; les icônes ont leurs conditions Font Awesome ; [licence](https://fontawesome.com/license/free) |
| `materialdesignicons-webfont.ttf` | Material Design Icons | Apache License 2.0 ; [dépôt MDI](https://github.com/Templarian/MaterialDesign) |

Les fichiers de licence complets des projets tiers doivent être conservés
avec toute redistribution qui les utilise. La provenance exacte de chaque
copie binaire importée depuis V3 doit être vérifiée avant une version publiée.

## Images et shaders

Les images BGRA/JPEG et les shaders présents dans `assets/` proviennent du
travail V3 ou du développement V4. Leur statut de copyright et leur licence
de redistribution doivent être confirmés individuellement avant de publier
une release ou un paquet binaire. Le firmware de bring-up n'en a besoin que
pour le framebuffer BGRA de test ; les shaders ne sont pas chargés par la
Jupiter 2 à ce stade.

## Documentation SpacemiT / K3

Les documents matériels importés sous `docs/jupiter2/` restent des documents
de référence du fabricant et sont conservés pour le développement. Ils ne sont
pas une œuvre originale du projet V4. Vérifier leurs conditions de
redistribution auprès de la source avant de créer une release qui les inclut.
