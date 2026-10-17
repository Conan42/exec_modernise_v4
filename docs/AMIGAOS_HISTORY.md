# AmigaOS — histoire et philosophie d’un système qui aurait dû être le meilleur

Ce chapitre présente pourquoi AmigaOS possédait, dès 1985, une avance
architecturale remarquable, et pourquoi cette avance n’a pas suffi à en faire
le système dominant.

## Un pionnier du multimédia et du temps réel

Avant que le mot « multimédia » ne devienne courant, l’Amiga réunissait déjà
graphisme animé, audio numérique, vidéo, entrée utilisateur et multitâche dans
une même machine. Le terme est aujourd’hui daté, mais il décrit une ambition à
réhabiliter : faire coopérer plusieurs flux continus avec une latence faible.

Exec apportait une forte réactivité grâce aux priorités de tâches, aux signaux,
aux interruptions et aux échanges par messages. Il s’agissait d’un système
orienté **temps réel souple** (réponse prévisible et rapide), pas d’un système
temps réel dur garantissant mathématiquement chaque échéance. Cette distinction
doit être conservée dans la V2 : viser la latence bornée sans promettre une
garantie que le matériel ou le scheduler ne peuvent fournir.

## 1. Naissance en 1985

L’Amiga 1000 est lancé en 1985. Carl Sassenrath conçoit Exec comme un noyau
compact orienté tâches, messages et ressources partagées, adapté à une machine
aux ressources limitées mais dotée de capacités graphiques et audio avancées.

## 2. Organisation

```text
Applications
├── Workbench
└── applications utilisateur

Libraries
├── exec.library
├── intuition.library
├── graphics.library
└── autres services

Devices
├── input.device
├── audio.device
├── timer.device
└── autres périphériques

Exec
├── tâches
├── mémoire
├── interruptions
└── IPC par messages
```

Workbench est une application système au-dessus d’Exec, Intuition et Graphics.
Il n’est pas nécessaire pour lancer une application ou ouvrir un Screen privé.

## 3. Le clin d’œil Atari TOS et la MAO

L’autre grande famille de l’époque est l’Atari ST (1985) et son **TOS**
(*The Operating System*), construit autour de GEMDOS et de l’interface GEM.
L’Atari ST proposait des ports MIDI intégrés, un coût accessible et une
réactivité appréciée des musiciens. Il a ainsi marqué la naissance de la MAO
moderne : séquenceurs, éditeurs et premiers studios numériques ont été
développés sur ST, notamment Cubase et Notator.

TOS était généralement mono-tâche et moins modulaire qu’AmigaOS, mais cette
simplicité réduisait les surprises dans un séquenceur MIDI. AmigaOS offrait un
multitâche et des capacités audio/vidéo plus riches ; Atari TOS offrait une
chaîne MIDI immédiatement pratique et économique. Les deux ont contribué à
faire entrer l’informatique dans les studios de la génération 1985–1990.

| Système | Atout historique pour les musiciens | Limite principale |
|---|---|---|
| AmigaOS | multimédia, audio/vidéo, multitâche, échantillonnage | environnement MIDI moins intégré |
| Atari TOS/ST | MIDI intégré, séquençage, simplicité, coût | multitâche et multimédia plus limités |

La V2 retient les deux leçons : l’architecture modulaire et multimédia
d’AmigaOS, ainsi que la latence et la simplicité d’un chemin audio/MIDI dédié.

## 4. Video Toaster et LightWave : le multimédia professionnel

Le **Video Toaster** de NewTek, lancé au début des années 1990 pour Amiga,
illustre le passage du multimédia ludique au multimédia professionnel. Cette
solution réunissait sur une même machine des fonctions de commutation vidéo,
incrustation, titrage, effets et production télévisuelle, en s’appuyant sur les
capacités graphiques et d’extension de l’Amiga.

**LightWave 3D** est né dans cet écosystème comme logiciel de modélisation,
d’animation et de rendu associé au Toaster, avant de devenir un produit
autonome. Il a démontré qu’un ordinateur personnel pouvait produire des images
de qualité broadcast et des effets 3D complexes à une époque où ces outils
étaient normalement réservés aux studios spécialisés.

Video Toaster et LightWave ne prouvent pas qu’AmigaOS était sans limites : ils
reposaient sur du matériel et des extensions dédiés. Ils montrent en revanche
la pertinence de sa philosophie : un noyau léger, des Devices spécialisés, des
Libraries accessibles et une application capable d’exploiter directement le
pipeline multimédia.

## 5. Philosophie de Carl Sassenrath

- multitâche préemptif sur une machine peu puissante ;
- processus légers adaptés aux travaux qui doivent partager un même contexte ;
- communication par messages plutôt que dépendance à des appels globaux ;
- noyau et services suffisamment petits pour rester compréhensibles ;
- Devices remplaçables et Libraries accessibles aux applications ;
- séparation entre politique du bureau et services système ;
- exploitation directe des capacités graphiques et audio du matériel.

Cette philosophie explique la sensation de rapidité et de liberté de l’Amiga.

## 6. Limites historiques

L’absence initiale de protection mémoire, la sécurité limitée, la dépendance au
matériel custom et un écosystème commercial fragile ont freiné son évolution.
« Le système qui devait gagner » est une appréciation historique, pas une
conclusion technique démontrable.

AmigaOS était également conçu autour d’un modèle principalement mono-cœur :
son multitâche préemptif permettait d’alterner rapidement les tâches, mais ne
signifiait pas qu’elles s’exécutaient simultanément sur plusieurs cœurs. La V2
conserve l’efficacité du modèle de tâches et l’étend au multicœur/SMP avec des
files par hart, une affinité CPU, des migrations contrôlées, des IPI et une
synchronisation explicite des ressources partagées.

## 7. Héritage pour Exec Modernisé V2

La V2 reprend le petit Exec indépendant, l’IPC explicite, les processus légers,
les contrats Libraries/Devices, le Workbench optionnel et les applications
directes. Elle ajoute protection mémoire, permissions, versionnement ABI,
backends remplaçables, récupération sur erreur et préparation au multicœur,
avec synchronisation et propriété explicites des ressources partagées. Le
socle portable du scheduler V2 sélectionne déjà les tâches par priorité,
répartit les tâches non épinglées entre les harts et refuse les migrations
contraires à l’affinité déclarée.

## 8. La synthèse que nous construisons

L’ambition d’Exec Modernisé V2 est de réunir deux héritages qui étaient
séparés :

```text
Video Toaster + LightWave
→ vidéo professionnelle, 3D, composition et rendu

Atari ST + TOS + MIDI
→ séquençage, audio réactif et contrôle direct par les musiciens
```

La V2 vise une plateforme où ces usages coexistent dans un même système :
pipeline GPU triple-buffered, audio/MIDI à latence bornée, réseau, applications
directes et bureaux interchangeables. La différence décisive est la base
moderne : isolation mémoire, contrats versionnés, sécurité, backpressure et
récupération sur erreur.

Autrement dit, nous construisons le système multimédia que l’Amiga et l’Atari
ST annonçaient, avec les garanties qu’ils ne pouvaient pas encore fournir.

## 9. Une plateforme RISC-V dédiée

Cette synthèse est destinée à une plateforme **RISC-V** moderne. L’ISA ouverte
permet de contrôler le démarrage, les niveaux de privilège, la mémoire et les
extensions sans dépendre d’un fournisseur de processeur fermé. Le SoC (par
exemple Jupiter 2) fournit ensuite les blocs concrets : GPU/DPU, audio,
contrôleur mémoire, stockage et réseau.

Dans ce chapitre, **hart** désigne un *Hardware Thread*, soit une unité
d’exécution RISC-V indépendante. La Jupiter 2 et QEMU avec `-smp 8` exposent
ici huit harts ; le mot ne garantit pas qu’un futur cœur physique ne puisse pas
exposer plusieurs harts via SMT.

Cette abstraction ouvre la voie à des configurations futures : plus de harts
sur les processeurs RVA23, des cœurs réservés à l’audio ou au GPU, une
affinité dynamique et un équilibrage de charge. Exec doit interroger la
topologie fournie par la plateforme et activer tous les harts disponibles ; la
limite actuelle de 64 harts est une capacité de conception du prototype, pas
une limite conceptuelle de RISC-V.

Le démarrage QEMU valide ce principe avec huit harts : un hart principal est
initialisé, puis les sept harts secondaires sont amenés en ligne. Chaque hart
possède ensuite un état de cycle de vie et peut être arrêté proprement. Cette
séquence prépare les futures topologies sans supposer que le hart 0 est le
seul cœur utile.

La règle d’architecture est stricte :

```text
contrats V2 portables
        ↓
adaptateur RISC-V / SoC
        ↓
GPU, audio, réseau et affichage du matériel
```

RISC-V ne rend pas automatiquement le système temps réel ou multimédia. Il faut
encore valider les interruptions, le MMU, la cohérence cache/DMA, les fences GPU
et les latences audio sur le SoC réel. Ces propriétés sont donc des contrats à
mesurer, pas des hypothèses.
