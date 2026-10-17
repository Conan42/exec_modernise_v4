# Architecture V4 — premier jalon

Ce document décrit les frontières du projet, le chemin de bring-up de la
Jupiter 2 et la trajectoire vers l'interface V4. Les diagrammes Mermaid sont
volontairement centrés sur les contrats et les flux observables ; ils ne
déclarent pas opérationnels les composants encore en cours de portage.

## Carte du projet

```mermaid
flowchart LR
    subgraph Host[PC hôte Linux]
        CLI[host/jupiter2ctl\nUART + serveur TFTP]
        Assets[assets/\nimages, fontes, shaders]
        Tests[cargo test\nvalidation portable]
    end

    subgraph Workspace[Workspace Rust V4]
        Protocol[protocol\ncontrats no_std]
        Exec[exec\nSMP, IPI, SBI, MMU]
        FS[filesystem\nELF64 + MFFS]
        Bonjour[firmware/bonjour\nimage bring-up]
    end

    subgraph Board[Milk-V Jupiter 2]
        UBoot[U-Boot SPL + U-Boot]
        DRAM[LPDDR5\nframebuffer BGRA]
        DPU[DPU1 / Saturn\nRDMA + DMMU + cmdlist]
        DP[DP1 / USB-C Alt Mode]
        Adapter[adaptateur USB-C → HDMI]
        Monitor[HP Compaq LA2006x]
    end

    CLI -->|UART 115200 8N1| UBoot
    CLI -->|TFTP RRQ| UBoot
    Protocol --> Bonjour
    Exec --> Protocol
    FS --> Exec
    Bonjour -->|go 0x140000000| DRAM
    DRAM --> DPU --> DP --> Adapter --> Monitor
    Assets -. futur chemin hôte .-> Tests
    Tests -.-> Protocol

    classDef validated fill:#d9fbe5,stroke:#16803c,color:#0b3d1e;
    classDef pending fill:#fff2cc,stroke:#b7791f,color:#5f370e;
    class UBoot,DRAM,DPU,DP,CLI,Protocol,FS,Exec validated;
    class Adapter,Monitor,Assets,Bonjour pending;
```

La chaîne UART/TFTP, la mémoire, le firmware `bonjour`, les registres DPU/DP,
le link training et les marqueurs de scanout sont observés sur la carte. Le
moniteur reste sans signal avec la chaîne USB-C vers HDMI ; le jalon
`SPLASH_OK` n'est donc pas déclaré.

## Flux de démarrage et de validation

```mermaid
sequenceDiagram
    autonumber
    participant Dev as Développeur
    participant Host as jupiter2ctl
    participant U as U-Boot
    participant Mem as DRAM
    participant FW as bonjour
    participant D as DPU1 / DP1
    participant HP as HP LA2006x

    Dev->>Host: ouvre /dev/ttyUSB0
    Dev->>U: reset puis prompt =>
    Host->>U: setenv ipaddr / serverip
    Host->>U: tftpboot framebuffer BGRA
    U->>Host: 5 760 000 octets transférés
    U->>Mem: écrit 0x102000000
    Host->>U: tftpboot bonjour-jupiter2.bin
    U->>Host: 30 861 octets transférés
    Host->>U: go 0x140000000
    U->>FW: saute à l'image RISC-V
    FW->>Host: UART_OK → DMMU → CMDLIST → LINK_TRAIN_OK
    FW->>D: configure timing 1600×900, RBR, 2 voies
    D->>HP: flux DP via adaptateur USB-C/HDMI
    HP-->>Dev: pas de signal observé
```

## Chemin vidéo matériel

```mermaid
flowchart TD
    Image[Image BGRA8\n1600×900 · stride 6400] --> DRAM[DRAM\n0x102000000]
    DRAM --> DMMU[Table DMMU\n1407 pages + garde]
    DMMU --> RDMA[RDMA0\nformat Saturn 7]
    RDMA --> CMP[Composer / cmdlist\nreload RDMA + composer]
    CMP --> TMG[TMG0\n108 MHz · DMT 1600×900]
    TMG --> DP[DP1\nRBR 1,62 Gb/s · 2 lanes]
    DP --> AUX[AUX + link training\nDPCD 1.2 · TU 64]
    AUX --> USB[USB-C DP Alt Mode]
    USB --> HDMI[Adaptateur USB-C / HDMI]
    HDMI --> Sink[HP Compaq LA2006x]

    UART[UART DEBUG] -. journaux .-> DMMU
    Sink -. résultat actuel : pas de signal .-> Failure[(SPLASH_OK absent)]

    classDef ok fill:#d9fbe5,stroke:#16803c,color:#0b3d1e;
    classDef open fill:#fff2cc,stroke:#b7791f,color:#5f370e;
    class Image,DRAM,DMMU,RDMA,CMP,TMG,DP,AUX,UART ok;
    class USB,HDMI,Sink,Failure open;
```

## Frontières des composants

```mermaid
classDiagram
    class Jupiter2Ctl {
        +monitor(serial, baud)
        +tftp_serve(bind, root)
    }
    class Protocol {
        <<no_std>>
        +version
        +wire_contracts
    }
    class ExecV4 {
        +scheduler SMP
        +IPI and SBI
        +MMU and contexts
    }
    class FilesystemV4 {
        +parse_elf64()
        +parse_mffs()
        +lookup()
    }
    class Bonjour {
        +UART diagnostics
        +DPU1 scanout
        +DP1 link training
    }
    class HostAssets {
        +BGRA images
        +TTF fonts
        +GLSL and SPIR-V
    }
    class TacticalGlobe {
        <<reference project>>
        +UI contracts
        +Vulkan organization
    }

    Jupiter2Ctl ..> Bonjour : loads by TFTP
    Jupiter2Ctl ..> Protocol : future commands
    ExecV4 ..> Protocol : shared contract
    FilesystemV4 ..> ExecV4 : image validation
    Bonjour ..> Protocol : future transport
    HostAssets ..> TacticalGlobe : reference assets and UI
    HostAssets ..> Jupiter2Ctl : host-side tools only
```

## États du bring-up

```mermaid
stateDiagram-v2
    [*] --> UART
    UART --> TFTP: RRQ validée
    TFTP --> Firmware: go 0x140000000
    Firmware --> Memory: UART_OK / DRAM_OK
    Memory --> DPU: DMMU_STAGE_OK
    DPU --> DPLink: CMDLIST_ENABLE_OK
    DPLink --> Scanout: LINK_TRAIN_OK + DP_MSA_STAGE_OK
    Scanout --> Visual: signal vidéo observé
    Scanout --> Investigation: écran sans signal
    Visual --> [*]
    Investigation --> [*]

    note right of Investigation
        État actuel du HP LA2006x.
        Les marqueurs UART passent,
        mais SPLASH_OK manque.
    end note
```

## Décision de séparation

V4 ne copie pas le workspace V3. V3 reste la source de contrats et de
validation QEMU/Venus ; V4 commence avec une surface réduite pour pouvoir
valider la carte réelle avant de réintroduire l'UI et le rendu.

```text
host/jupiter2ctl ── UART DEBUG ── Milk-V Jupiter 2
       │
       └──────────── TFTP RRQ ─── bootloader de la carte

assets ─── ressources UI / shaders ─── outil hôte, après validation Vulkan

protocol (no_std)
       │
firmware/bonjour ─── image de test, après validation du profil de boot

exec-v4 ─── scheduler SMP / contextes / IPI / SBI / MMU
    │
    └── filesystem-v4 (validation ELF/MFFS)
```

La liaison électrique UART DEBUG est en **logique 1,8 V**. GND est commun et
TXD/RXD sont croisés ; un adaptateur 3,3 V ne doit pas être branché directement.
La couche hôte configure ensuite l'UART en 8N1 sans contrôle de flux, comme la
liaison DEBUG déjà validée. Le serveur TFTP sert uniquement des noms relatifs sous sa
racine et implémente le transfert `octet` par blocs de 512 octets avec ACK et
retransmissions bornées.

## Preuve de démarrage obtenue

La liaison UART a été validée avec un FTDI FT232R sur `/dev/ttyUSB0`, en
logique 1,8 V. Le boot observé est celui d'une carte SpacemiT K3 Pico ITX :
U-Boot SPL 2022.10, U-Boot 2022.10, 16 GiB de LPDDR5, puis Bianbu 4.0.7 avec
Linux 6.18.3-generic sur UFS. La partition `/boot` est `/dev/sda2` et contient
le noyau, l'initrd et le DTB `spacemit/6.18.3-generic/k3-pico-itx.dtb`.

Pendant la phase d'inspection, le point d'arrêt était volontairement situé
avant Linux. Comme la touche d'interruption U-Boot n'avait pas été identifiée,
`/boot/env_k3.txt` contenait
temporairement `bootdelay=5` et `bootcmd=echo V4_BOOT_HALTED`. Cette méthode
produit le prompt U-Boot `=>` et conserve les copies
`env_k3.txt.boot_bak` et `env_k3.txt.bootdelay_bak`. Elle permet d'inspecter
U-Boot et de préparer le réseau sans modifier encore l'image V4.

Cette preuve établit la chaîne console, bootloader, mémoire, stockage et
réseau. U-Boot a reçu `jupiter2-test.bin` depuis `192.168.1.131` avec
l'adresse Jupiter `192.168.1.132`, à l'adresse mémoire `0x102000000`. Elle ne
valide pas encore le protocole V4 ni l'exécution de `firmware/bonjour`.

Le jalon suivant a maintenant validé l'exécution de `firmware/bonjour` : le
splash BGRA est chargé à `0x102000000`, le firmware à `0x140000000`, et les
jalons `UART_OK`, `DRAM_OK` et `SPLASH_RAM_READ` arrivent sur la console. Le
scanout DPU/DisplayPort reste séparé de cette preuve d'exécution.

La sonde suivante a validé la lecture MMIO de la sortie utilisée par le
Pico-ITX : DPU1 à `0xC0440000` et DP1 à `0xCAC88000`. Elle renvoie la
signature Saturn `0x08040030`. Le futur backend d'affichage suivra le pilote
Linux dans l'ordre power-domain → clocks/resets → DMMU/RDMA/cmdlist → DP AUX
et timings ; l'adresse du framebuffer seule ne constitue pas une séquence de
scanout.

Les assets importés de V3 restent dans le chemin hôte. Les fontes et modules
SPIR-V ne sont pas embarqués dans `firmware/bonjour` et ne sont pas servis par
TFTP. Le seul fichier temporairement copié dans `tftp-root` est le splash BGRA,
pour le test de framebuffer décrit ci-dessous. L'utilisation UI complète
attend la validation du pilote RGX/Vulkan et du contrat de ressources.

Le premier jalon graphique V4 suit un chemin plus court : UART pour le journal,
TFTP pour déposer le framebuffer BGRA en DRAM, puis DPU/DisplayPort pour le
scanout. RGX et les shaders restent hors de ce jalon. Le framebuffer
`1920×1080×4` occupe `8 294 400` octets avec un stride de `7680`.

Le firmware a validé la préparation du timing Saturn DP1 à `DPU1 + 0x51200`
pour 1920×1080@60, sans activer `fm_timing_en`. Le prochain point matériel est
le plane RDMA0 en BGRA8888, suivi du mapping DMMU et de la cmdlist avant le
link training DP.

Le plane RDMA0 est maintenant préparé et relu sur la carte : adresse
`0x102000000`, stride `7680`, format Saturn `7` (`BGRA8888`) et surface
1920×1080. La couche composer reste désactivée tant que le DMMU et la cmdlist
ne sont pas portés.

La séquence matérielle est volontairement progressive :

```text
UART log → DRAM accessible → image TFTP en mémoire
         → DPU plane brut → timings DisplayPort → splash visible
```

Le chargement en mémoire est testable dès maintenant avec l'adresse
`0x102000000`, déjà validée par U-Boot. La présentation bare-metal attend la
validation des clocks, resets, power domains, mapping DMMU/IOMMU et registres
du DPU. Aucun accès registre graphique n'est ajouté sur la seule base des
adresses documentées.

## Suite de portage

1. construire une image V4 connue et la déposer dans `tftp-root/` ;
2. vérifier son chargement TFTP sans l'exécuter ;
3. intégrer le point d'entrée et le linker Jupiter 2 dans `firmware/bonjour` ;
4. ajouter le transport de commandes V4 sur UART ;
5. porter les contrats `RenderPlan`, surfaces et z-order de V3 ;
6. intégrer egui/Vulkan de TacticalGlobe dans l'outil de validation ;
7. seulement ensuite ouvrir le backend RGX/Vulkan puis V2D/DPU.

Le backend matériel ne sera marqué opérationnel qu'après validation des
registres, DMA, caches, IRQ et fences sur la carte.
