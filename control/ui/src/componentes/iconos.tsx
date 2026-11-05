import { HugeiconsIcon } from "@hugeicons/react";
import {
  Activity01Icon,
  Alert02Icon,
  AlertCircleIcon,
  ArrowLeft01Icon,
  BatteryCharging01Icon,
  BatteryFullIcon,
  BatteryLowIcon,
  BatteryMedium01Icon,
  BookOpen01Icon,
  CableIcon,
  CallIcon,
  Cancel01Icon,
  CheckmarkCircle02Icon,
  CircleDashedIcon,
  CornerDownLeftIcon,
  DashboardSquare01Icon,
  Download01Icon,
  EyeIcon,
  EyeOffIcon,
  FlaskConicalIcon,
  HashtagIcon,
  Layers01Icon,
  Loading03Icon,
  Message01Icon,
  PauseIcon,
  PlayIcon,
  PlugSocketIcon,
  PlusIcon,
  Refresh01Icon,
  Rotate01Icon,
  RotateLeft01Icon,
  ScaleIcon,
  SmartPhone01Icon,
  Square01Icon,
  Upload01Icon,
  UsbConnected01Icon,
  Wallet01Icon,
  Search01Icon,
  Copy01Icon,
  Tick02Icon,
  Key01Icon,
  Megaphone01Icon,
  ArrowRight01Icon,
  Xls01Icon,
  HelpCircleIcon,
  ArrowUp01Icon,
  ArrowDown01Icon,
  Delete02Icon,
  TestTube01Icon,
  PencilEdit02Icon,
} from "@hugeicons/core-free-icons";

type Props = { className?: string };
type Datos = typeof Activity01Icon;

/** Envuelve un icono de Hugeicons con la misma firma que usaban los de lucide (`className` fija el tamaño). */
const hacer = (icono: Datos) =>
  function Icono({ className = "size-4" }: Props) {
    return <HugeiconsIcon icon={icono} strokeWidth={1.75} className={className} />;
  };

export const Activity = hacer(Activity01Icon);
export const ArrowLeft = hacer(ArrowLeft01Icon);
export const BatteryCharging = hacer(BatteryCharging01Icon);
export const BatteryFull = hacer(BatteryFullIcon);
export const BatteryLow = hacer(BatteryLowIcon);
export const BatteryMedium = hacer(BatteryMedium01Icon);
export const BookOpen = hacer(BookOpen01Icon);
export const Cable = hacer(CableIcon);
export const CheckCircle2 = hacer(CheckmarkCircle02Icon);
export const CircleAlert = hacer(AlertCircleIcon);
export const CircleCheck = hacer(CheckmarkCircle02Icon);
export const CircleDashed = hacer(CircleDashedIcon);
export const CornerDownLeft = hacer(CornerDownLeftIcon);
export const Download = hacer(Download01Icon);
export const Eye = hacer(EyeIcon);
export const EyeOff = hacer(EyeOffIcon);
export const FileSpreadsheet = hacer(Xls01Icon);
export const FlaskConical = hacer(FlaskConicalIcon);
export const Hash = hacer(HashtagIcon);
export const Layers = hacer(Layers01Icon);
export const MessageSquare = hacer(Message01Icon);
export const Pause = hacer(PauseIcon);
export const PhoneCall = hacer(CallIcon);
export const Play = hacer(PlayIcon);
export const PlugZap = hacer(PlugSocketIcon);
export const Plus = hacer(PlusIcon);
export const RefreshCw = hacer(Refresh01Icon);
export const RotateCcw = hacer(RotateLeft01Icon);
export const RotateCw = hacer(Rotate01Icon);
export const Scale = hacer(ScaleIcon);
export const Smartphone = hacer(SmartPhone01Icon);
export const Square = hacer(Square01Icon);
export const TriangleAlert = hacer(Alert02Icon);
export const Unplug = hacer(PlugSocketIcon);
export const Upload = hacer(Upload01Icon);
export const Usb = hacer(UsbConnected01Icon);
export const Wallet = hacer(Wallet01Icon);
export const X = hacer(Cancel01Icon);
export const Search = hacer(Search01Icon);
export const Copy = hacer(Copy01Icon);
export const Check = hacer(Tick02Icon);
export const Key = hacer(Key01Icon);
export const Megaphone = hacer(Megaphone01Icon);
export const ArrowRight = hacer(ArrowRight01Icon);
export const ArrowUp = hacer(ArrowUp01Icon);
export const ArrowDown = hacer(ArrowDown01Icon);
export const Trash = hacer(Delete02Icon);
export const TestTube = hacer(TestTube01Icon);
export const Pencil = hacer(PencilEdit02Icon);

const HGira = hacer(Loading03Icon);
export const Loader2 = ({ className = "size-4 animate-spin" }: Props) => <HGira className={className} />;

const MAPA = { panel: DashboardSquare01Icon, lotes: Layers01Icon, ussd: HashtagIcon, recetas: BookOpen01Icon, sms: Message01Icon, actividad: Activity01Icon, manual: HelpCircleIcon };
export type NombreIcono = keyof typeof MAPA;
export function Icono({ nombre, className = "size-5" }: { nombre: NombreIcono; className?: string }) {
  return <HugeiconsIcon icon={MAPA[nombre]} strokeWidth={1.75} className={className} />;
}
