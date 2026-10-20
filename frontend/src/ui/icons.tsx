import type { ComponentPropsWithoutRef, ComponentType } from "react";
import {
  ArrowBendUpLeft as PhosphorArrowBendUpLeft,
  ArrowClockwise as PhosphorArrowClockwise,
  ArrowCounterClockwise as PhosphorArrowCounterClockwise,
  ArrowLeft as PhosphorArrowLeft,
  ArrowRight as PhosphorArrowRight,
  ArrowSquareOut as PhosphorArrowSquareOut,
  ArrowUp as PhosphorArrowUp,
  ArrowsClockwise as PhosphorArrowsClockwise,
  At as PhosphorAt,
  Bell as PhosphorBell,
  BellSlash as PhosphorBellSlash,
  BracketsCurly as PhosphorBracketsCurly,
  Brain as PhosphorBrain,
  CalendarDots as PhosphorCalendarDots,
  Camera as PhosphorCamera,
  CaretDown as PhosphorCaretDown,
  CaretLeft as PhosphorCaretLeft,
  CaretRight as PhosphorCaretRight,
  ChartBar as PhosphorChartBar,
  ChatCircle as PhosphorChatCircle,
  Check as PhosphorCheck,
  CheckCircle as PhosphorCheckCircle,
  CircleNotch as PhosphorCircleNotch,
  Cloud as PhosphorCloud,
  Code as PhosphorCode,
  Copy as PhosphorCopy,
  Cpu as PhosphorCpu,
  Crown as PhosphorCrown,
  CurrencyDollar as PhosphorCurrencyDollar,
  DeviceMobile as PhosphorDeviceMobile,
  Devices as PhosphorDevices,
  DotsThree as PhosphorDotsThree,
  DotsThreeVertical as PhosphorDotsThreeVertical,
  DownloadSimple as PhosphorDownloadSimple,
  FileArrowDown as PhosphorFileArrowDown,
  FileArrowUp as PhosphorFileArrowUp,
  FileText as PhosphorFileText,
  FloppyDisk as PhosphorFloppyDisk,
  Folder as PhosphorFolder,
  GameController as PhosphorGameController,
  Gear as PhosphorGear,
  Globe as PhosphorGlobe,
  Hash as PhosphorHash,
  Image as PhosphorImage,
  Key as PhosphorKey,
  Laptop as PhosphorLaptop,
  Lifebuoy as PhosphorLifebuoy,
  Lightning as PhosphorLightning,
  Link as PhosphorLink,
  MagnifyingGlass as PhosphorMagnifyingGlass,
  Monitor as PhosphorMonitor,
  Package as PhosphorPackage,
  Palette as PhosphorPalette,
  PaperPlaneTilt as PhosphorPaperPlaneTilt,
  Paperclip as PhosphorPaperclip,
  Pause as PhosphorPause,
  PauseCircle as PhosphorPauseCircle,
  PencilSimple as PhosphorPencilSimple,
  Play as PhosphorPlay,
  Plugs as PhosphorPlugs,
  Plus as PhosphorPlus,
  PlusCircle as PhosphorPlusCircle,
  Prohibit as PhosphorProhibit,
  Pulse as PhosphorPulse,
  PushPin as PhosphorPushPin,
  Radio as PhosphorRadio,
  Robot as PhosphorRobot,
  ShieldCheck as PhosphorShieldCheck,
  ShieldWarning as PhosphorShieldWarning,
  SignIn as PhosphorSignIn,
  SignOut as PhosphorSignOut,
  Smiley as PhosphorSmiley,
  Sparkle as PhosphorSparkle,
  SpeakerHigh as PhosphorSpeakerHigh,
  SpeakerSlash as PhosphorSpeakerSlash,
  Square as PhosphorSquare,
  Star as PhosphorStar,
  SquaresFour as PhosphorSquaresFour,
  StopCircle as PhosphorStopCircle,
  Terminal as PhosphorTerminal,
  Trash as PhosphorTrash,
  UploadSimple as PhosphorUploadSimple,
  User as PhosphorUser,
  UserCheck as PhosphorUserCheck,
  UserMinus as PhosphorUserMinus,
  UserPlus as PhosphorUserPlus,
  Users as PhosphorUsers,
  WarningCircle as PhosphorWarningCircle,
  WifiHigh as PhosphorWifiHigh,
  Wrench as PhosphorWrench,
  X as PhosphorX,
  XCircle as PhosphorXCircle,
  type IconProps,
} from "@phosphor-icons/react";

/**
 * Single owner of the application icon set.
 *
 * Every icon is a solid Phosphor glyph ("fill" for shapes with an interior,
 * "bold" for pure strokes). Call sites keep the Lucide-era names and props;
 * `strokeWidth` is accepted for compatibility and ignored. `data-icon` names
 * the icon in kebab-case so UI tests do not depend on library class names.
 */
export type AppIconProps = Omit<ComponentPropsWithoutRef<"svg">, "ref"> & {
  size?: number | string;
  strokeWidth?: number | string;
  weight?: never;
};

export type AppIcon = ComponentType<AppIconProps>;

type PhosphorGlyph = ComponentType<IconProps>;
type Weight = "fill" | "bold";

function createIcon(name: string, Glyph: PhosphorGlyph, weight: Weight, outlineWeight: Weight = "bold"): AppIcon {
  function AppIconComponent({ size = 24, strokeWidth: _strokeWidth, fill, ...props }: AppIconProps) {
    const outlined = fill === "none";

    return (
      <Glyph
        size={size}
        weight={outlined ? outlineWeight : weight}
        data-icon={name}
        {...(outlined || fill === undefined ? {} : { fill })}
        {...props}
      />
    );
  }

  AppIconComponent.displayName = name;
  return AppIconComponent;
}

export const Activity = createIcon("activity", PhosphorPulse, "bold");
export const ArrowLeft = createIcon("arrow-left", PhosphorArrowLeft, "bold");
export const ArrowRight = createIcon("arrow-right", PhosphorArrowRight, "bold");
export const ArrowUp = createIcon("arrow-up", PhosphorArrowUp, "bold");
export const AtSign = createIcon("at-sign", PhosphorAt, "bold");
export const Ban = createIcon("ban", PhosphorProhibit, "fill");
export const BarChart3 = createIcon("bar-chart-3", PhosphorChartBar, "fill");
export const Bell = createIcon("bell", PhosphorBell, "fill");
export const BellOff = createIcon("bell-off", PhosphorBellSlash, "fill");
export const Bot = createIcon("bot", PhosphorRobot, "fill");
export const Braces = createIcon("braces", PhosphorBracketsCurly, "bold");
export const Brain = createIcon("brain", PhosphorBrain, "fill");
export const CalendarDays = createIcon("calendar-days", PhosphorCalendarDots, "fill");
export const Camera = createIcon("camera", PhosphorCamera, "fill");
export const Check = createIcon("check", PhosphorCheck, "bold");
export const ChevronDown = createIcon("chevron-down", PhosphorCaretDown, "bold");
export const ChevronLeft = createIcon("chevron-left", PhosphorCaretLeft, "bold");
export const ChevronRight = createIcon("chevron-right", PhosphorCaretRight, "bold");
export const CircleAlert = createIcon("circle-alert", PhosphorWarningCircle, "fill");
export const CircleCheck = createIcon("circle-check", PhosphorCheckCircle, "fill");
export const CirclePause = createIcon("circle-pause", PhosphorPauseCircle, "fill");
export const CirclePlus = createIcon("circle-plus", PhosphorPlusCircle, "fill");
export const CircleStop = createIcon("circle-stop", PhosphorStopCircle, "fill");
export const CircleX = createIcon("circle-x", PhosphorXCircle, "fill");
export const Cloud = createIcon("cloud", PhosphorCloud, "fill");
export const Code2 = createIcon("code-2", PhosphorCode, "bold");
export const Copy = createIcon("copy", PhosphorCopy, "fill");
export const Crown = createIcon("crown", PhosphorCrown, "fill");
export const CornerUpLeft = createIcon("corner-up-left", PhosphorArrowBendUpLeft, "bold");
export const Cpu = createIcon("cpu", PhosphorCpu, "fill");
export const Dollar = createIcon("dollar", PhosphorCurrencyDollar, "fill");
export const Download = createIcon("download", PhosphorDownloadSimple, "bold");
export const ExternalLink = createIcon("external-link", PhosphorArrowSquareOut, "bold");
export const FileDown = createIcon("file-down", PhosphorFileArrowDown, "fill");
export const FileText = createIcon("file-text", PhosphorFileText, "fill");
export const FileUp = createIcon("file-up", PhosphorFileArrowUp, "fill");
export const FileUser = createIcon("file-user", PhosphorFileText, "fill");
export const Folder = createIcon("folder", PhosphorFolder, "fill");
export const Gamepad2 = createIcon("gamepad-2", PhosphorGameController, "fill");
export const Globe = createIcon("globe", PhosphorGlobe, "fill");
export const Hash = createIcon("hash", PhosphorHash, "bold");
export const Image = createIcon("image", PhosphorImage, "fill");
export const ImageIcon = Image;
export const KeyRound = createIcon("key-round", PhosphorKey, "fill");
export const Laptop = createIcon("laptop", PhosphorLaptop, "fill");
export const LayoutDashboard = createIcon("layout-dashboard", PhosphorSquaresFour, "fill");
export const LifeBuoy = createIcon("life-buoy", PhosphorLifebuoy, "fill");
export const Link2 = createIcon("link-2", PhosphorLink, "fill");
export const LoaderCircle = createIcon("loader-circle", PhosphorCircleNotch, "bold");
export const LogIn = createIcon("log-in", PhosphorSignIn, "bold");
export const LogOut = createIcon("log-out", PhosphorSignOut, "bold");
export const MessageSquare = createIcon("message-square", PhosphorChatCircle, "fill");
export const Monitor = createIcon("monitor", PhosphorMonitor, "fill");
export const MonitorSmartphone = createIcon("monitor-smartphone", PhosphorDevices, "fill");
export const MoreHorizontal = createIcon("more-horizontal", PhosphorDotsThree, "bold");
export const MoreVertical = createIcon("more-vertical", PhosphorDotsThreeVertical, "bold");
export const PackageOpen = createIcon("package-open", PhosphorPackage, "fill");
export const Palette = createIcon("palette", PhosphorPalette, "fill");
export const Paperclip = createIcon("paperclip", PhosphorPaperclip, "bold");
export const Pause = createIcon("pause", PhosphorPause, "fill");
export const Pencil = createIcon("pencil", PhosphorPencilSimple, "fill");
export const Pin = createIcon("pin", PhosphorPushPin, "fill");
export const Play = createIcon("play", PhosphorPlay, "fill");
export const Plus = createIcon("plus", PhosphorPlus, "bold");
export const Radio = createIcon("radio", PhosphorRadio, "fill");
export const RefreshCw = createIcon("refresh-cw", PhosphorArrowsClockwise, "bold");
export const RotateCcw = createIcon("rotate-ccw", PhosphorArrowCounterClockwise, "bold");
export const RotateCw = createIcon("rotate-cw", PhosphorArrowClockwise, "bold");
export const Save = createIcon("save", PhosphorFloppyDisk, "fill");
export const Search = createIcon("search", PhosphorMagnifyingGlass, "bold");
export const Send = createIcon("send", PhosphorPaperPlaneTilt, "fill");
export const ServerOff = createIcon("server-off", PhosphorPlugs, "fill");
export const Settings = createIcon("settings", PhosphorGear, "fill");
export const ShieldCheck = createIcon("shield-check", PhosphorShieldCheck, "fill");
export const ShieldQuestion = createIcon("shield-question", PhosphorShieldWarning, "fill");
export const Smartphone = createIcon("smartphone", PhosphorDeviceMobile, "fill");
export const Smile = createIcon("smile", PhosphorSmiley, "fill");
export const Sparkles = createIcon("sparkles", PhosphorSparkle, "fill");
export const Star = createIcon("star", PhosphorStar, "fill");
export const Square = createIcon("square", PhosphorSquare, "fill");
export const Terminal = createIcon("terminal", PhosphorTerminal, "fill");
export const Trash2 = createIcon("trash-2", PhosphorTrash, "fill");
export const Upload = createIcon("upload", PhosphorUploadSimple, "bold");
export const User = createIcon("user", PhosphorUser, "fill");
export const UserCheck = createIcon("user-check", PhosphorUserCheck, "fill");
export const UserMinus = createIcon("user-minus", PhosphorUserMinus, "fill");
export const UserPlus = createIcon("user-plus", PhosphorUserPlus, "fill");
export const UserRound = createIcon("user-round", PhosphorUser, "fill");
export const UserX = createIcon("user-x", PhosphorUserMinus, "fill");
export const Users = createIcon("users", PhosphorUsers, "fill");
export const Volume2 = createIcon("volume-2", PhosphorSpeakerHigh, "fill");
export const VolumeX = createIcon("volume-x", PhosphorSpeakerSlash, "fill");
export const Wifi = createIcon("wifi", PhosphorWifiHigh, "fill");
export const Wrench = createIcon("wrench", PhosphorWrench, "fill");
export const X = createIcon("x", PhosphorX, "bold");
export const Zap = createIcon("zap", PhosphorLightning, "fill");
