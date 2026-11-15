import type { LucideIcon } from "lucide-react"
import {
  Armchair,
  Baby,
  Bath,
  BedDouble,
  BedSingle,
  BookOpen,
  Briefcase,
  Car,
  CookingPot,
  DoorOpen,
  Dumbbell,
  Flower2,
  Gamepad2,
  House,
  Shirt,
  Sofa,
  Toilet,
  Trees,
  Tv,
  Utensils,
  Warehouse,
  WashingMachine,
  Waves,
  Wine,
} from "lucide-react"

/** The icons a room can have (Lucide names, as kept by the server). */
export const ROOM_ICONS: Record<string, LucideIcon> = {
  house: House,
  sofa: Sofa,
  tv: Tv,
  armchair: Armchair,
  "bed-double": BedDouble,
  "bed-single": BedSingle,
  baby: Baby,
  "cooking-pot": CookingPot,
  utensils: Utensils,
  bath: Bath,
  toilet: Toilet,
  shirt: Shirt,
  briefcase: Briefcase,
  "book-open": BookOpen,
  "gamepad-2": Gamepad2,
  dumbbell: Dumbbell,
  "washing-machine": WashingMachine,
  "door-open": DoorOpen,
  warehouse: Warehouse,
  car: Car,
  trees: Trees,
  "flower-2": Flower2,
  waves: Waves,
  wine: Wine,
}

/** Words of a room's name and the icon they suggest (French first, then English and Spanish). */
const GUESSES: [RegExp, string][] = [
  [/salon|séjour|sejour|living|lounge|sala/i, "sofa"],
  [/bébé|bebe|enfant|baby|nursery|niño/i, "baby"],
  [/parent|principale|master|matrimonial/i, "bed-double"],
  [/chambre|bedroom|dormitorio|habitación/i, "bed-single"],
  [/cuisine|kitchen|cocina/i, "cooking-pot"],
  [/salle à manger|salle a manger|repas|dining|comedor/i, "utensils"],
  [/salle de bain|sdb|bain|douche|bath|shower|baño/i, "bath"],
  [/toilette|wc|toilet|aseo/i, "toilet"],
  [/dressing|vestiaire|closet|vestidor/i, "shirt"],
  [/bureau|office|despacho|travail/i, "briefcase"],
  [/biblioth|lecture|library/i, "book-open"],
  [/jeu|jeux|game|juego/i, "gamepad-2"],
  [/sport|gym/i, "dumbbell"],
  [/buanderie|laverie|laundry|lavadero|cellier/i, "washing-machine"],
  [/entrée|entree|couloir|hall|palier|entrance|pasillo/i, "door-open"],
  [/garage|parking/i, "car"],
  [/cave|grenier|combles|atelier|cellar|attic|garaje|trastero/i, "warehouse"],
  [/piscine|pool/i, "waves"],
  [
    /jardin|extérieur|exterieur|terrasse|garden|outdoor|jardín|terraza/i,
    "trees",
  ],
  [/véranda|veranda/i, "flower-2"],
]

/** The icon of a room: chosen, else guessed from its name, else a house. */
export function roomIconName(room: { name: string; icon?: string | null }) {
  if (room.icon && ROOM_ICONS[room.icon]) return room.icon
  return GUESSES.find(([re]) => re.test(room.name))?.[1] ?? "house"
}

export function roomIcon(room: { name: string; icon?: string | null }) {
  return ROOM_ICONS[roomIconName(room)]
}
