# Architecture V4 — premier jalon

## Décision de séparation

V4 ne copie pas le workspace V3. V3 reste la source de contrats et de
validation QEMU/Venus ; V4 commence avec une surface réduite pour pouvoir
valider la carte réelle avant de réintroduire l'UI et le rendu.

```text
host/jupiter2ctl ── UART DEBUG ── Milk-V Jupiter 2
       │
       └──────────── TFTP RRQ ─── bootloader de la carte

protocol (no_std)
       │
firmware/hello ─── image de test, après validation du profil de boot
```

La couche hôte configure l'UART en 8N1 sans contrôle de flux, comme la liaison
DEBUG déjà validée. Le serveur TFTP sert uniquement des noms relatifs sous sa
racine et implémente le transfert `octet` par blocs de 512 octets avec ACK et
retransmissions bornées.

## Suite de portage

1. vérifier la console UART réelle et noter la séquence de boot ;
2. servir une image déjà connue par TFTP et confirmer son chargement sur la
   carte ;
3. intégrer le point d'entrée et le linker Jupiter 2 dans `firmware/hello` ;
4. ajouter le transport de commandes V4 sur UART ;
5. porter les contrats `RenderPlan`, surfaces et z-order de V3 ;
6. intégrer egui/Vulkan de TacticalGlobe dans l'outil de validation ;
7. seulement ensuite ouvrir le backend RGX/Vulkan puis V2D/DPU.

Le backend matériel ne sera marqué opérationnel qu'après validation des
registres, DMA, caches, IRQ et fences sur la carte.

