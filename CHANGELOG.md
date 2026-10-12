# Changelog

## 1.4.0

- **Physique structurelle** : stabilité par chemin de support (portée selon le matériau),
  piliers porteurs dans les bâtiments, mobilier indépendant de la structure. Les planchers,
  façades et porte-à-faux trop longs tombent.
- **Feu** : combustion et propagation (bois, feuillage, tissu), flammes visibles, brûlures.
- **Poussière et fumée** à chaque écroulement, impact et explosion.
- **Préréglages graphiques** Bas / Moyen / Élevé / Ultra / Personnalisé, avec anticrénelage,
  particules et bloom réglables.
- **Son** : effets normalisés, portée spatiale plus longue, nouveaux sons (grondement,
  impact lourd, crépitement, pose de bloc). **Musique** générative et dossier `music`.
- **Écran de chargement** : la ville est générée en arrière-plan, le jeu démarre quand le
  terrain proche est prêt (plus de saccades à l'arrivée).
- **Mémoire** : maillages compacts (24 octets par sommet, sans copie CPU), chargés selon la
  distance d'affichage, matériaux des morceaux en palette 4 bits. ~1,1 Go au lieu de ~2,8 Go.
- Journal du serveur à l'heure de la machine (plus d'UTC).

## 1.3.0

- **Physique** : un bâtiment dont la base est détruite tombe en bloc. L'ensemble détaché devient
  un corps rigide (masse, centre de masse, inertie, gravité 9,81 m/s²), heurte le monde, se
  casse à l'impact, se scinde, puis se fige. Plus de limite de taille : les ensembles
  flottants n'« oublient » plus de tomber.
- **Solo sur serveur** : le solo lance un serveur privé local ; physique, dégâts, argent,
  portes, blocs et sauvegardes sont les mêmes en solo et en ligne.
- **Serveur autoritaire** : inventaire, argent, munitions, rechargement et cadence de tir
  sont validés côté serveur ; les explosions doivent correspondre à une bombe armée.
- **Anti-triche** : contrôle de vitesse, de traversée de murs et de vol, correction de
  position, expulsion progressive, limite de connexions par IP, bannissement.
- **Sauvegardes** : fichiers `.bgw` contenant tout le monde (gravats et corps compris) ;
  sauvegarde / chargement en solo et sur serveur (automatique et à l'arrêt).
- **server.conf** : ~25 réglages (armes interdites, construction, portes, dégâts de chute,
  effondrement, argent de départ, sauvegarde automatique, anti-triche…).
- **Journal du serveur** style Minecraft (`logs/latest.log`), console avec commandes.
- **AppImage** : plus seulement portable, elle s'installe (menu des applications,
  commande `boomground`).
- **Rendu** : occlusion ambiante, texture de voxel (grain, biseau, joints) avec mipmaps,
  filtrage anisotrope, MSAA 4×, HDR, bloom, tonemapping et étalonnage.
- Protocole réseau v4.

## 1.2.0

- Carte ~30 fois plus grande (845 × 845 m), stockage par morceaux alloués à la demande,
  chargement progressif du maillage.
- Quartiers : centre-ville de tours, quartiers mixtes, banlieue.
- Intérieurs meublés (tables, chaises, bureaux, canapés, étagères, plafonniers) et
  mobilier urbain (lampadaires, bancs, bouches d'incendie, poubelles, feux tricolores).
- Portes interactives (`F`), synchronisées en multijoueur.
- Outil de construction *Blocs* (9 matériaux), synchronisé en multijoueur.
- Physique : effondrement de ce qui n'est plus relié au sol, masse des gravats, écrasement.
- Points de vie, mort, ragdoll, caméra à la troisième personne, réapparition.
- Nouvelles armes : Pioche, Revolver .44, Pistolet-mitrailleur, Minigun, Railgun.
- Réglages : distance d'affichage, ombres, bruitages, secousses, balancement, FPS, viseur.
- Interface : l'aide permanente des touches est retirée du HUD.
- Serveur dédié `BoomGround-server` (Windows, Linux) avec `server.conf` généré au premier
  lancement.
- Protocole réseau v3 (incompatible avec les versions précédentes).
- Nouvelle licence (source disponible, contributions acceptées, copie interdite).

## 1.1.0

- Premier build publié : jeu en un seul fichier (Windows `.exe`, Linux `.AppImage`).
