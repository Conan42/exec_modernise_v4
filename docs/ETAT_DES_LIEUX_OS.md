# État des lieux des systèmes d’exploitation

> Document repris de la V2 et conservé comme état historique des choix de
> conception. Pour l'état matériel courant de V4, consulter
> [`../PROGRESS.md`](../PROGRESS.md) et [`../JUPITER2_BRINGUP.md`](../JUPITER2_BRINGUP.md).

Ce document sert de base comparative au livre V2. Les avantages et limites
sont évalués du point de vue architecture, fiabilité, contrôle utilisateur,
portabilité et efficacité ; il ne s’agit pas d’un classement marketing.

## 1. Comparaison synthétique

| Système | Forces | Limites structurelles |
|---|---|---|
| Linux | noyau ouvert, modularité, forte automatisation, diversité matérielle | fragmentation, interfaces variables, maintenance parfois complexe, expérience inégale |
| macOS | intégration matériel/logiciel, Unix, distribution maîtrisée, bonne UX | matériel fermé, contrôle Apple, extensibilité limitée, dépendance fournisseur |
| Android | très grande variété matérielle, modèle applicatif sandboxé, écosystème vaste | fragmentation des versions, mises à jour dépendantes des fabricants, services Google fréquents |
| iOS | sandbox et modèle de sécurité cohérents, mises à jour centralisées, UX homogène | plateforme fermée, restrictions fortes, distribution et matériel contrôlés |
| Windows | compatibilité logicielle et matérielle exceptionnelle, écosystème professionnel | héritage et complexité, télémétrie, surface d’attaque, cohérence et maintenance variables |
| AmigaOS | Exec compact, multitâche préemptif, processus légers, IPC par messages, Libraries/Devices modulaires, faible coût système | protection mémoire et sécurité initiales faibles, dépendance au matériel custom, modèle historiquement mono-cœur et concurrence limitée |

Aucun système ne maximise simultanément ouverture, sécurité, simplicité,
compatibilité et contrôle utilisateur. La V2 doit choisir explicitement ses
priorités au lieu d’hériter de compromis implicites.

Dans la terminologie RISC-V utilisée ici, un **hart** est un *Hardware Thread*
(thread matériel), c’est-à-dire une unité d’exécution indépendante visible par
le système. Pour la Jupiter 2 et QEMU `-smp 8`, nous parlons de 8 harts, qui
correspondent ici à 8 cœurs d’exécution ; un futur processeur SMT pourrait
exposer plusieurs harts par cœur physique.

## 2. Linux

Linux sépare un noyau monolithique modulaire, les pilotes, les bibliothèques et
les espaces utilisateur. Son ouverture favorise le portage, l’audit et
l’automatisation. En contrepartie, les distributions, ABI et systèmes de
paquets divergent. La V2 retient l’ouverture des contrats et la validation
reproductible, mais évite de rendre chaque couche dépendante d’une distribution.

## 3. macOS

macOS combine un noyau XNU, des composants BSD et une intégration matérielle
étroite. Cette maîtrise rend l’expérience cohérente et les mises à jour
prévisibles. Elle réduit cependant la liberté matérielle et le contrôle de
l’utilisateur. La V2 retient la cohérence des interfaces, sans imposer un
matériel propriétaire.

## 4. Android et iOS

Android privilégie l’échelle et la diversité ; iOS privilégie l’homogénéité et
le contrôle. Les deux isolent les applications, mais l’utilisateur dépend du
cycle de mise à jour et des politiques de leur fournisseur. La V2 retient le
principe de capacités et de permissions explicites, sans dépendre d’un magasin
central.

## 5. Windows

Windows conserve une compatibilité remarquable avec plusieurs décennies de
logiciels et de matériels. Cette force produit aussi une grande dette de
compatibilité, des couches historiques et une surface de configuration vaste.
La V2 retient la compatibilité par contrats versionnés, plutôt que par
exceptions accumulées dans chaque couche.

## 6. Multithreading et multicœur

Le multitâche préemptif ne se limite pas à faire progresser plusieurs
processus : Exec doit aussi pouvoir créer des processus légers (threads) au
sein d’un même processus. Ces threads partagent l’espace mémoire et les
ressources autorisées du processus, tout en conservant leur propre pile, leur
contexte CPU, leur état d’ordonnancement et leur identifiant. Cette distinction
permet par exemple de laisser l’interface réactive pendant qu’un worker
décode un flux audio, sans transformer chaque opération en nouveau processus.

La création de processus reprend maintenant le contrat utile de la V1 sans
réintroduire son couplage implicite : `CreateProcess` reçoit le nom ExecFS
NUL-terminal en mémoire utilisateur, puis `ProcessImageResolver` sépare la
résolution du programme de sa publication dans Exec. Le runtime attribue un
identifiant borné et crée le thread principal. Le chemin complet
`dispatch_runtime_with_process_image()` charge les segments ELF dans des
frames, réserve une pile privée, attache le contexte principal et publie
l’espace avec son `satp` de manière transactionnelle. Il reste à monter un
vrai volume MFFS dans le kernel pour l’exercer au boot ; le handle de haut
niveau `dos.library` reste un service à intégrer.

Le socle `dos-v2` est maintenant compilable : il normalise les chemins en
refusant les NUL et les sorties de racine, borne les tailles, distingue les
handles de fichier/répertoire/volume/processus et transporte un contexte CLI
ainsi que `stdin/stdout/stderr`. Cette façade ne possède toujours ni le
stockage ni le scheduler ; elle passera par Exec pour consommer le catalogue
MFFS et lancer les images validées.

Le répertoire `filesystem/` n’est plus seulement déclaratif : le crate
`filesystem-v2` valide les ELF64 RISC-V statiques et lit le catalogue MFFS V1.
`MffsProcessImageResolver` consomme ce lecteur dans Exec, vérifie le checksum,
le type APP, les permissions et l’ELF, puis fournit le lancement du processus.
Son chargeur logique réserve les frames, recopie les segments, met le BSS à
zéro et construit un plan transactionnel ; le runtime publie ce plan avec un
`satp` validé, un contexte CPU et une pile privée. Le profil QEMU possède le
backend d’écriture identité et le handler RV64 sait raccorder un store fault
COW à la copie, au PTE et à `sfence.vma`. Le montage de volume MFFS et le fork
COW matériel restent les validations d’intégration manquantes.

Le multicœur ajoute une seconde dimension : plusieurs harts/cœurs peuvent
exécuter simultanément des tâches distinctes. Il impose donc des règles
d’atomicité, de synchronisation, de propriété des ressources et de réveil des
consommateurs ; la préemption seule ne suffit pas. La V2 prévoit un support
SMP progressif et portable. Dans l’état actuel, le bring-up sait démarrer les
8 harts QEMU et traiter un IPI SSIP sur un hart secondaire, mais le scheduler
général n’est pas encore SMP de production : cette limite doit rester explicite
dans les documents et les tests.
Un essai QEMU `-accel tcg,thread=multi -smp 8` s’est arrêté après
`BMHCAETPOSQ` sans atteindre le scénario complet ; la référence reproductible
reste donc `thread=single` et cette exécution concurrente demeure à corriger.

### Remédiation prévue

La limitation SMP est un chantier identifié, et non une propriété définitive
du système. La migration de l’ordonnanceur se fera par étapes. Les deux
premières sont maintenant couvertes par le modèle portable de
`exec/src/scheduler.rs` :

1. donner à chaque hart un état scheduler local et une file `Ready` protégée ;
2. ajouter une file globale ou un rééquilibrage borné entre les files locales ;
3. utiliser des verrous courts et des IPI (*Inter-Processor Interrupt*,
   interruptions inter-harts) pour les réveils, préemptions et migrations ; le
   transport SBI et le traitement SSIP secondaire sont validés, mais leur
   intégration au scheduler général reste à finaliser ;
4. définir l’affinité CPU des tâches sensibles, notamment GPU et audio ;
5. synchroniser les mappings partagés et les invalidations TLB ;
6. valider la préemption, la migration, les files concurrentes et les threads
   partageant un même processus sur plusieurs cœurs.

Le premier jalon SMP portable concerne les tâches indépendantes et valide déjà
la répartition, la priorité et l’affinité. Le second jalon est maintenant validé :
les files locales sont rééquilibrées par une migration transactionnelle d’un
thread `Ready` avec son `TaskContext`, et les candidats épinglés sont ignorés
au profit d’un thread `Any`. Sous QEMU avec 8 vCPU, le tick du primaire
déplace réellement ce worker puis adresse un IPI au hart destinataire ; `Y`
confirme cette migration. Le transport IPI matériel et le traitement SSIP d’un
secondaire restent également validés. Une première réduction de contention est
acquise : le trampoline lit des snapshots atomiques par hart et le timer SBI
est armé hors du verrou global. Le contrat `exec/src/smp.rs` regroupe
désormais dans un état local atomique par hart le thread courant, le snapshot de
contexte, l’état de boot et la demande de rescheduling ; aucun de ces champs ne
nécessite le verrou du registre Exec. L’exécution simultanée générale nécessite
encore la partition sûre des registres de threads, contextes et opérations
transversales ; le verrou global n’est donc pas déclaré supprimé. Les
threads d’un même processus ne seront autorisés à migrer entre cœurs qu’après
validation de la propriété mémoire, des atomiques et des primitives de
synchronisation.

La phase suivante est préparée dans `exec/src/scheduler.rs` par le contrat
`HartQueueSnapshot`, qui fixe la vue bornée d’un domaine `online + ReadyQueue`
par hart, distinct des métadonnées globales d’identité et d’affinité. La
représentation reste pour l’instant protégée par `RuntimeCell` : l’accès
concurrent réel à ces domaines attend leur synchronisation dédiée, afin de ne
pas introduire de régression dans le bring-up QEMU.

`SchedulerOperationScope` classe maintenant les opérations locales et les
migrations inter-harts. `block_thread()` et `wake_thread()` transmettent le
hart propriétaire aux primitives de file ; une incohérence est refusée avant
mutation. La synchronisation concurrente dédiée reste la prochaine étape.

Sur la mémoire, le clonage COW est maintenant raccordé au registre d’espaces :
les pages utilisateur inscriptibles sont partagées en lecture seule et leurs
références sont comptées transactionnellement. Le trait `PageCopier` formalise
la copie physique fournie par la plateforme ;
`resolve_cow_write_with_copy()` publie la page privée uniquement après copie
réussie et restaure le plan, les références et l’allocateur en cas d’erreur.
Le traitement du fault d’écriture et le branchement de ce contrat dans le
trap matériel restent à implémenter.

La frontière de trap est toutefois maintenant explicitement décodée dans
`exec/src/syscall.rs` : un `scause=15` provenant de l’U-Mode produit
`UserStorePageFault(stval)`, tandis qu’un fault supervisor reste rejeté. Une
adresse `stval` non alignée est normalisée au début de sa page avant la
résolution COW. Le kernel doit encore appeler cette résolution depuis son
handler avec l’espace processus, les références, l’allocateur et le backend
physique appropriés.
La façade runtime et le registre d’espaces savent désormais router cette
résolution vers le processus concerné ; le handler matériel doit encore
fournir les objets mémoire et publier la nouvelle table Sv39 après succès.
Les vues Sv39 savent désormais remplacer une feuille déjà présente sans
effacer les tables existantes, et le profil QEMU fournit un `PageCopier` basé
sur son mapping identité. Le handler n’appelle encore pas ce chemin dans le
boot nominal : il manque la publication d’un espace enfant COW et son cycle
de vie matériel.
Le cycle de création Exec est désormais couvert par
`create_process_with_cow_address_space()` : l’enfant et son espace COW sont
publiés ensemble, avec rollback du processus et du parent si le `satp` ou le
clonage est refusé. Le handler RV64 possède maintenant la séquence fault →
copie → PTE → `sfence.vma` pour l’espace actif ; un fork COW matériel reste à
ajouter au scénario QEMU.
Le reaping COW retire désormais les références de l’enfant ou du parent de
façon transactionnelle et signale les frames dont le dernier détenteur a
disparu. La restitution physique effective reste effectuée par le backend.
Le backend `FrameAllocator` fournit maintenant cette restitution de manière
bornée et vérifie la propriété ainsi que l’absence de double libération.

### Vérification du correctif `satp == 0` — 27 août 2026

La recompilation complète de UILab et du kernel avec `venus-pixel-probe` est
validée. Le smoke test QEMU atteint Venus, le splash, UILab et le premier
`PollInput` sans boucle `user-load` ni crash U-Mode. Aucun clic n’est simulé par
ce test court : la validation interactive des gadgets et du déplacement de
fenêtre reste à faire. Les seuls messages de compilation sont des
avertissements `unsafe` imbriqués dans le kernel.

`gadtools.library` ne demande plus une frame complète au `PointerDown` : la
capture et l’état pressé sont mémorisés, puis l’activation au `PointerUp`
publie une seule frame finale. Les 72 tests de `libraries-v2` passent. Cette
réduction supprime le double rendu par clic ; la présentation Venus encore
synchrone reste le prochain point de latence à traiter dans le worker
`graphics.library`.

Pour les choix exclusifs, deux cases peuvent changer au même clic. Elles sont
maintenant dessinées dans une seule passe delta ; l'ID journalisé est celui de
la première case modifiée.

Le log série identifie désormais les actions GadTools :
`[gadtools] action=checkbox`, `plus`, `moins`, `slider`, `autre` ou `full`.
Les phases `Down`/`Up` restent journalisées par `input.device`, et
`render=58` marque la fin de la présentation Venus. Cette trace sépare le
délai de réception du clic du délai de rendu.

Chaque action porte aussi `id=<NodeId>`, par exemple
`[gadtools] action=checkbox id=14 render=58`. Cet ID est celui du `SceneTree`
Libraries, pas un identifiant QEMU.

Le crash apparu après l'ajout du cache d'ID venait d'une copie par valeur du
grand tableau de plan Venus, qui débordait la pile. Les accesseurs utilisent
maintenant des références ; QEMU redémarre correctement. La frame initiale
utilise `id=65535`, valeur réservée signifiant « aucun gadget ».

Les timestamps RISC-V sont présents dans le log : `time` sur les événements
`Down`/`Up`, puis `click_time`, `draw_start`, `draw_end`, `draw_ticks` et
`input_to_draw` sur l'action GadTools. Ce sont des ticks bruts monotones ;
`click_time=0` ne concerne que la frame initiale.

Le débit Graphics est maintenant mesuré dans le log avec
`[graphics.library] fps=... frames=... ticks=...`. La mesure couvre frames
complètes et deltas, et n'apparaît qu'après environ une seconde d'activité.

Un essai de lots Venus de 16 primitives a été retiré : il provoque un échec
au stage 51 sur la première frame UILab. La limite validée reste donc huit
primitives par lot. Le prochain travail GPU doit porter sur une vraie
soumission asynchrone avec fence et ressources séparées, dans le worker
Graphics.

Une première intégration du service différé sur tick timer a été retirée :
QEMU ne franchissait plus le chemin UILab nominal, car Venus partage encore
son contexte et ses ressources avec le chemin de présentation synchrone. Le
profil stable est restauré et vérifié (`P`, `first-poll-call`,
`first-pointer`). Le worker asynchrone attend donc la séparation du contexte,
la fence non bloquante et la rétention explicite des surfaces.

Le sprite est désormais publié à chaque événement `Move` par
`cursor.library`, sans attendre quatre déplacements et sans redessiner la
scène. Cette latence est donc séparée du rendu Venus complet ; le contrat est
portable vers Jupiter 2.

Le délai des clics provenait des frames Venus déclenchées par le hover pendant
les déplacements. Le hover reste maintenant géré par `gadtools.library`, mais
un `Move` ne déclenche plus de présentation complète ; le sprite reste publié
immédiatement par `cursor.library`. Les tests et la compilation sont validés.

Le backend Venus regroupe désormais douze primitives par soumission du ring,
ce qui réduit les attentes synchrones par frame. QEMU démarre correctement
avec cette limite ; seize a été rejeté après un échec transport au stage 51.
La frame complète restant synchrone, la vraie cible suivante est le rendu
Graphics borné de la seule zone du gadget modifié.

Le premier delta de rendu est maintenant implémenté pour les CheckBox : une
activation isolée réutilise le scanout et ne redessine que le rectangle de la
case. Toute modification plus large repasse par la frame complète validée.
Les tests Devices et le smoke test QEMU passent.

La capacité du scheduler ne doit pas être confondue avec le nombre de cœurs
d’une machine donnée : V2 prévoit une capacité portable jusqu’à 64 harts,
mais la plateforme fournit le nombre réellement présent et tous les harts
disponibles doivent être activés par défaut. Pour l’instant, QEMU est la seule
cible exécutable et doit être lancé avec `-smp 8` ; la Jupiter 2 à 8 cœurs est
seulement la cible matérielle prévue. Le profil peut limiter ce nombre
uniquement pour un test explicite. L’affinité utilise un masque de 64 bits ;
les structures par hart sont bornées et doivent être allouées en fonction du
nombre effectivement détecté afin de ne pas consommer inutilement la mémoire.
Une extension à 128 ou 256 harts sera envisagée uniquement si une plateforme
réelle l’exige.

## 7. Principes retenus pour Exec Modernisé V2

La V2 reprend :

1. un Exec petit et indépendant des Devices ;
2. des processus légers préemptifs partageant explicitement les ressources de
   leur processus ;
3. un ordonnancement multicœur/SMP avec affinité et migration contrôlées ;
4. des contrats Libraries/Devices stables et versionnés ;
5. un IPC explicite et borné ;
6. un Workbench optionnel ;
7. des applications directes et des bureaux alternatifs ;
8. la rapidité sans sacrifier l’isolation mémoire, la sécurité et la reprise
   sur erreur.

Elle ne reprend pas l’absence de protection mémoire, les handles globaux non
   vérifiés ou les dépendances implicites au matériel.
Un IPI est un signal direct envoyé par un hart à un autre hart. Il permet de
réagir sans attendre le prochain tick timer : réveil, rescheduling, demande
d’invalidation TLB ou arrêt propre. Les IPI servent au contrôle du scheduler ;
ils ne remplacent ni l’IPC par messages ni les synchronisations protégeant la
mémoire partagée.

Le log a confirmé que l'entrée était rapide, mais que le focus modifié en
même temps que la case forçait à tort le fallback `full`. Le détecteur accepte
maintenant ce cas et conserve le rendu delta de la `CheckBox` avec son ID.
