# Contribuer à Exec Modernisé V4

Exec Modernisé V4 est un projet Rust expérimental qui porte progressivement
les contrats d'Exec Modernisé vers la Jupiter 2. Les contributions peuvent
viser le code portable, l'outillage hôte, la documentation ou le bring-up
matériel.

## Avant une modification

Lire [`ARCHITECTURE.md`](ARCHITECTURE.md) et [`PROGRESS.md`](PROGRESS.md). Une
modification matérielle doit indiquer le registre ou le document de référence,
la trace UART obtenue et le résultat observé sur le matériel.

Ne jamais publier de mot de passe, de clé privée, de token ou de configuration
réseau personnelle. Les adresses du réseau de laboratoire doivent rester des
exemples remplaçables.

## Validation locale

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
git diff --check
```

Pour construire l'image de bring-up :

```bash
./scripts/build-bonjour.sh
```

Le script produit `tftp-root/bonjour-jupiter2.bin`. Le contenu de
`tftp-root/` est généré localement et n'est pas destiné à être commité.

## Matériel Jupiter 2

La console de debug utilise un UART **1,8 V**, 115200 8N1, sans contrôle de
flux. Le niveau électrique doit être vérifié avant de brancher un adaptateur.
Les transferts TFTP et les manipulations U-Boot sont décrits dans
[`JUPITER2_BRINGUP.md`](JUPITER2_BRINGUP.md).

Une trace sans preuve visuelle doit rester décrite comme telle. Les marqueurs
`LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et `SCANOUT_STAGE_OK` ne suffisent pas à
déclarer `SPLASH_OK`.

## Documentation et assets

Les nouveaux diagrammes d'architecture utilisent Mermaid intégré dans GitHub.
Les assets importés doivent être accompagnés de leur provenance et de leur
licence dans [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
