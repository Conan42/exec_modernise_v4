# Progression V4

## 16 octobre 2026 — V4-0 : socle de bring-up

Le projet `/home/mich/RustProjects/exec_modernise_v4` est créé comme workspace
Rust indépendant. RustRover peut l'ouvrir directement avec son `Cargo.toml`.

La contrainte électrique de la console DEBUG est fixée à **1,8 V logique**.
Elle ne doit pas être confondue avec la liaison 3,3 V de la Jupiter 1.

Présent :

- `host/jupiter2ctl` avec moniteur UART `115200 8N1` sans contrôle de flux ;
- serveur TFTP `octet`, RRQ, blocs de 512 octets, ACK et retransmissions ;
- `protocol`, crate `no_std` pour les futures trames de commande ;
- `exec-v4` et `filesystem-v4` repris de la V3, avec scheduler multi-hart,
  états SMP, IPI, SBI, contextes et contrats MMU ;
- image `firmware/hello`, volontairement activée seulement avec `--features
  board` tant que le linker et le profil de boot Jupiter 2 ne sont pas fixés ;
- configurations RustRover pour l'information, l'UART et le TFTP.

Validation : `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo
run -p jupiter2ctl -- info` et un smoke TFTP UDP local sont passés. Le test
d'ouverture UART a atteint le périphérique FTDI mais le compte courant n'est
pas membre du groupe Linux `dialout`; l'accès réel nécessite donc la règle
d'accès déjà utilisée pour la liaison de la carte.

## Suite immédiate

1. rétablir l'accès utilisateur à `/dev/ttyUSB0` et capturer le boot UART ;
2. déposer une image connue dans `tftp-root/` et confirmer le RRQ du
   bootloader ;
3. fixer l'adresse de chargement, le linker et le point d'entrée de l'image
   V4 ;
4. seulement après cette preuve, porter les contrats UI/Vulkan de TacticalGlobe
   et les contrats `RenderPlan`/surfaces de V3.
