# Exec Modernisé V4

V4 est le nouveau projet de bring-up pour la Milk-V Jupiter 2. Il reprend les
contrats utiles de V1, V2 et V3, et prendra TacticalGlobe comme référence pour
l'organisation de l'UI egui et du rendu Vulkan côté hôte et côté outil de
validation.

Le premier jalon est volontairement limité à la communication avec la carte :

- UART DEBUG : `/dev/ttyUSB0`, `115200 8N1`, sans contrôle de flux ;
- TFTP : serveur de fichiers pour les requêtes RRQ du bootloader ;
- protocole partagé `no_std` pour les futures commandes binaires ;
- image firmware minimale séparée, activée seulement lorsque le profil de boot
  Jupiter 2 et son adresse de chargement sont confirmés.

Le projet est indépendant du dépôt TacticalGlobe et ne modifie pas son pipeline
de relief. Il ne déclare pas encore le backend RGX/Vulkan ou V2D/DPU matériel :
les documents V3 indiquent que ces blocs nécessitent d'abord une séquence
clocks, reset, power-domain, IOMMU, DMA et IRQ validée sur la carte.

## Ouvrir dans RustRover

Ouvrir le fichier `Cargo.toml` de ce répertoire. RustRover détecte le workspace
et le binaire `jupiter2ctl` automatiquement. Les configurations `Info V4`,
`Monitor UART Jupiter 2` et `Serveur TFTP Jupiter 2` sont fournies dans
`.idea/runConfigurations/`.

## Premier essai

```bash
cargo run -p jupiter2ctl -- info
cargo run -p jupiter2ctl -- monitor --serial /dev/ttyUSB0
cargo run -p jupiter2ctl -- tftp-serve --root tftp-root
```

Le port TFTP 69 nécessite généralement une capacité réseau ou un lancement
privilégié. Pour un test local sans privilège, utiliser un port supérieur à
1024 ; le bootloader qui attend le port standard doit, lui, recevoir le service
sur UDP 69.

L'état du travail et les preuves de validation sont suivis dans
[`PROGRESS.md`](PROGRESS.md).
