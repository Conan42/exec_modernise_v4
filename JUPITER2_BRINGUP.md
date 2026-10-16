# Bring-up Milk-V Jupiter 2

## Liaison actuelle — niveau logique 1,8 V

> **Ne pas utiliser 3,3 V. La liaison UART DEBUG de la Jupiter 2 est en
> logique 1,8 V.** Cette précision concerne le câblage électrique, pas
> seulement la configuration du terminal.

Le script historique `/home/mich/uart_connect.sh` utilise `/dev/ttyUSB0` à
`115200` bauds, `8N1`, sans contrôle de flux. V4 reprend exactement ces
paramètres dans `jupiter2ctl monitor`.

La liaison USB/UART DEBUG utilise donc un **niveau logique de 1,8 V** : GND est
commun, TXD de l'adaptateur va vers RXD de la carte et RXD de l'adaptateur va
vers TXD de la carte. Utiliser un adaptateur USB-UART compatible 1,8 V, ou un
convertisseur de niveau adapté. La documentation Jupiter 1 en 3,3 V ne doit
pas être réutilisée pour ce câblage.

## TFTP

Le bootloader est le client TFTP : il envoie une requête `RRQ` au PC. V4
fournit le serveur dans `host/jupiter2ctl`. La racine par défaut est
`tftp-root/`, qui doit être créée et recevoir explicitement l'image à servir.

```bash
mkdir -p tftp-root
cargo run -p jupiter2ctl -- tftp-serve --bind 0.0.0.0:69 --root tftp-root
```

Le serveur refuse les chemins absolus et les traversals `..`. Il ne sert que le
mode TFTP `octet`, avec des blocs de 512 octets et trois tentatives par bloc.

## État matériel

Les adresses documentées du K3 ne sont pas encore transformées en driver V4 :

- UART0 : `0xD401_7000` ;
- RGX : `0xCAC0_0000` ;
- V2D : `0xC010_0000` ;
- DPU0 : `0xC034_0000` ;
- DPU1 : `0xC044_0000` ;
- IOMMU : `0xC0F0_0000`.

Ces adresses seules ne suffisent pas à initialiser le matériel. Les clocks,
resets, power-domains, IOMMU, DMA et IRQ doivent être portés depuis le device
tree et validés sur la carte.
