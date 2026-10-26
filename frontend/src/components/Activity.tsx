import { Activity, Flower2, HandFist, HeartPulse, House, Mountain, Music, Sailboat, Snowflake, Sprout, Target, Trophy, Volleyball } from 'lucide-react';
import type { Exercise } from '@/lib/api';
import { cn } from '@/lib/utils';

/** 24 × 24 filled glyphs (Material Symbols, Apache 2.0); lucide has no running figure. */
const GLYPH = {
  run: 'M13.49 5.48c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm-3.6 13.9 1-4.4 2.1 2v6h2v-7.5l-2.1-2 .6-3c1.3 1.5 3.3 2.5 5.5 2.5v-2c-1.9 0-3.5-1-4.3-2.4l-1-1.6c-.4-.6-1-1-1.7-1-.3 0-.5.1-.8.1l-5.2 2.2v4.7h2v-3.4l1.8-.7-1.6 8.1-4.9-1-.4 2 7 1.4z',
  walk: 'M13.5 5.5c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zM9.8 8.9 7 23h2.1l1.8-8 2.1 2v6h2v-7.5l-2.1-2 .6-3C14.8 12 16.8 13 19 13v-2c-1.9 0-3.5-1-4.3-2.4l-1-1.6c-.4-.6-1-1-1.7-1-.3 0-.5.1-.8.1L6 8.3V13h2V9.6l1.8-.7',
  bike: 'M15.5 5.5c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zM5 12c-2.8 0-5 2.2-5 5s2.2 5 5 5 5-2.2 5-5-2.2-5-5-5zm0 8.5c-1.9 0-3.5-1.6-3.5-3.5s1.6-3.5 3.5-3.5 3.5 1.6 3.5 3.5-1.6 3.5-3.5 3.5zm5.8-10 2.4-2.4.8.8c1.3 1.3 3 2.1 5.1 2.1V9c-1.5 0-2.7-.6-3.6-1.5l-1.9-1.9c-.5-.4-1-.6-1.6-.6s-1.1.2-1.4.6L7.8 8.4c-.4.4-.6.9-.6 1.4 0 .6.2 1.1.6 1.4L11 14v5h2v-6.2l-2.2-2.3zM19 12c-2.8 0-5 2.2-5 5s2.2 5 5 5 5-2.2 5-5-2.2-5-5-5zm0 8.5c-1.9 0-3.5-1.6-3.5-3.5s1.6-3.5 3.5-3.5 3.5 1.6 3.5 3.5-1.6 3.5-3.5 3.5z',
  swim: 'M22 21c-1.11 0-1.73-.37-2.18-.64-.37-.22-.6-.36-1.15-.36-.56 0-.78.13-1.15.36-.46.27-1.07.64-2.18.64s-1.73-.37-2.18-.64c-.37-.22-.6-.36-1.15-.36-.56 0-.78.13-1.15.36-.46.27-1.08.64-2.19.64-1.11 0-1.73-.37-2.18-.64-.37-.23-.6-.36-1.15-.36s-.78.13-1.15.36c-.46.27-1.08.64-2.19.64v-2c.56 0 .78-.13 1.15-.36.46-.27 1.08-.64 2.19-.64s1.73.37 2.18.64c.37.23.59.36 1.15.36.56 0 .78-.13 1.15-.36.46-.27 1.08-.64 2.19-.64 1.11 0 1.73.37 2.18.64.37.22.6.36 1.15.36s.78-.13 1.15-.36c.45-.27 1.07-.64 2.18-.64s1.73.37 2.18.64c.37.23.59.36 1.15.36v2zm0-4.5c-1.11 0-1.73-.37-2.18-.64-.37-.22-.6-.36-1.15-.36-.56 0-.78.13-1.15.36-.45.27-1.07.64-2.18.64s-1.73-.37-2.18-.64c-.37-.22-.6-.36-1.15-.36-.56 0-.78.13-1.15.36-.45.27-1.07.64-2.18.64s-1.73-.37-2.18-.64c-.37-.22-.6-.36-1.15-.36s-.78.13-1.15.36c-.47.27-1.09.64-2.2.64v-2c.56 0 .78-.13 1.15-.36.45-.27 1.07-.64 2.18-.64s1.73.37 2.18.64c.37.22.6.36 1.15.36.56 0 .78-.13 1.15-.36.45-.27 1.07-.64 2.18-.64s1.73.37 2.18.64c.37.22.6.36 1.15.36s.78-.13 1.15-.36c.45-.27 1.07-.64 2.18-.64s1.73.37 2.18.64c.37.22.6.36 1.15.36v2zM8.67 12c.56 0 .78-.13 1.15-.36.46-.27 1.08-.64 2.19-.64 1.11 0 1.73.37 2.18.64.37.22.6.36 1.15.36s.78-.13 1.15-.36c.12-.07.26-.15.41-.23L10.48 5C8.93 3.45 7.5 2.99 5 3v2.5c1.82-.01 2.89.39 4 1.5l1 1-3.25 3.25c.31.12.56.27.77.39.37.23.59.36 1.15.36zM14 5.5a2.5 2.5 0 1 0 5 0 2.5 2.5 0 1 0-5 0z',
  gym: 'M20.57 14.86 22 13.43 20.57 12 17 15.57 8.43 7 12 3.43 10.57 2 9.14 3.43 7.71 2 5.57 4.14 4.14 2.71 2.71 4.14l1.43 1.43L2 7.71l1.43 1.43L2 10.57 3.43 12 7 8.43 15.57 17 12 20.57 13.43 22l1.43-1.43L16.29 22l2.14-2.14 1.43 1.43 1.43-1.43-1.43-1.43L22 16.29z',
};

type Glyph = keyof typeof GLYPH;

/** Icons for the groups the filled glyphs don't cover, from lucide (drawn as white strokes). */
const LUCIDE = {
  hike: Mountain,
  cardio: HeartPulse,
  calm: Flower2,
  dance: Music,
  ball: Volleyball,
  winter: Snowflake,
  water: Sailboat,
  fight: HandFist,
  aim: Target,
  garden: Sprout,
  home: House,
  sport: Trophy,
  other: Activity,
};
type Group = Glyph | keyof typeof LUCIDE;

/**
 * Every activity type of the Google Health API (Exercise.ExerciseType), by icon group.
 * A type missing here (a new one) gets the generic icon.
 */
const GROUPS: Record<Group, string> = {
  run: 'RUNNING TRAIL_RUN INCLINE_RUN TREADMILL TRACK_AND_FIELD WATER_JOGGING',
  walk: 'WALKING NORDIC_WALKING POWER_WALKING INCLINE_WALK TREADMILL_WALK STROLLER_WALK WALK_WITH_WEIGHTS RUCKING',
  bike: 'BIKING OUTDOOR_BIKE MOUNTAIN_BIKE ELECTRIC_BIKE STATIONARY_BIKE SPINNING ASSAULT_BIKE HAND_CYCLING UNICYCLING',
  swim: 'SWIMMING SWIMMING_POOL SWIMMING_OPEN_WATER SYNCHRONIZED_SWIMMING WATER_AEROBICS WATER_POLO DIVING SNORKELING SCUBA_DIVING',
  gym: 'WEIGHTLIFTING WEIGHTS FREE_WEIGHTS WEIGHT_MACHINES STRENGTH_TRAINING FUNCTIONAL_STRENGTH_TRAINING POWERLIFTING BODY_WEIGHT CALISTHENICS CROSSFIT CORE_TRAINING RESISTANCE_BANDS TRX CIRCUIT_TRAINING BOOTCAMP',
  hike: 'HIKING BACKPACKING ORIENTEERING SNOWSHOEING CLIMBING ROCK_CLIMBING INDOOR_CLIMBING',
  cardio: 'HIIT TABATA_WORKOUT INTERVAL_WORKOUT CARDIO_WORKOUT AEROBIC_WORKOUT ELLIPTICAL STAIRCLIMBER STEP_TRAINING JUMPING_ROPE ROWING_MACHINE CROSS_TRAINING EXERCISE_CLASS WORKOUT OUTDOOR_WORKOUT CARDIO_SCULPT FITNESS_GAMING TRAMPOLINE PARKOUR GYMNASTICS',
  calm: 'YOGA YOGA_BIKRAM YOGA_HATHA YOGA_POWER YOGA_VINYASA PILATES STRETCHING BARRE_CLASS TAI_CHI MEDITATE',
  dance: 'DANCING BALLET BALLROOM_DANCE BREAKDANCING HIP_HOP JAZZ_DANCE MODERN_DANCE TANGO ZUMBA CHEERLEADING MUSICAL_PERFORMANCE',
  ball: 'SOCCER BASKETBALL VOLLEYBALL VOLLEYBALL_BEACH WATER_VOLLEYBALL HANDBALL RUGBY FOOTBALL_AMERICAN FOOTBALL_AUSTRALIAN BASEBALL SOFTBALL CRICKET HOCKEY FIELD_HOCKEY LACROSSE POLO ULTIMATE_FRISBEE FRISBEE_PLAYING_GENERAL TENNIS SQUASH BADMINTON TABLE_TENNIS PADEL PICKELBALL RACQUETBALL RACKET_SPORTS',
  winter: 'SKIING CROSS_COUNTRY_SKI SNOWBOARDING SNOW_SPORT SNOWMOBILING ICE_SKATING SPEED_SKATING CURLING',
  water: 'ROWING CANOEING KAYAKING PADDLEBOARDING SAILING SURFING WINDSURFING KITESURFING WAKEBOARDING WATER_SKIING FOILING WATER_SPORT FISHING',
  fight: 'BOXING KICKBOXING MARTIAL_ARTS KARATE JIU_JITSU MUAY_THAI TAEKWONDO WRESTLING FENCING',
  aim: 'GOLF ARCHERY SHOOTING BILLIARDS BOWLING CROQUET HUNTING',
  garden: 'GARDENING MOWING_LAWN WEEDING HOEING SHOVELING',
  home: 'CLEANING HOUSEHOLD_CHORES CARPENTRY PAINTING',
  sport: 'SPORT MULTISPORT EQUESTRIAN_SPORTS SKATEBOARDING SKATING ROLLERBLADING ROLLER_SKATING SCOOTERING ELECTRIC_SCOOTER MOTOCROSS MOTORCYCLE PARAGLIDING SKYDIVING WHEELCHAIR',
  other: '',
};
const GROUP_OF = new Map(Object.entries(GROUPS).flatMap(([g, kinds]) => kinds.split(' ').filter(Boolean).map((k) => [k, g as Group])));

/** English names where "MOUNTAIN_BIKE" → "Mountain bike" would read oddly. */
const LABEL: Record<string, string> = {
  BIKING: 'Cycling',
  OUTDOOR_BIKE: 'Outdoor cycling',
  MOUNTAIN_BIKE: 'Mountain biking',
  ELECTRIC_BIKE: 'E-bike',
  STATIONARY_BIKE: 'Stationary bike',
  ASSAULT_BIKE: 'Air bike',
  HAND_CYCLING: 'Hand cycling',
  SWIMMING_POOL: 'Pool swimming',
  SWIMMING_OPEN_WATER: 'Open water swimming',
  TRAIL_RUN: 'Trail running',
  INCLINE_RUN: 'Incline running',
  INCLINE_WALK: 'Incline walking',
  TREADMILL_WALK: 'Treadmill walking',
  STROLLER_WALK: 'Walking with a stroller',
  WALK_WITH_WEIGHTS: 'Weighted walking',
  HIIT: 'HIIT',
  TRX: 'TRX',
  CROSSFIT: 'CrossFit',
  TABATA_WORKOUT: 'Tabata',
  INTERVAL_WORKOUT: 'Intervals',
  CARDIO_WORKOUT: 'Cardio',
  AEROBIC_WORKOUT: 'Aerobics',
  STAIRCLIMBER: 'Stair climber',
  BODY_WEIGHT: 'Bodyweight training',
  WEIGHTS: 'Weights',
  MEDITATE: 'Meditation',
  YOGA_BIKRAM: 'Bikram yoga',
  YOGA_HATHA: 'Hatha yoga',
  YOGA_POWER: 'Power yoga',
  YOGA_VINYASA: 'Vinyasa yoga',
  BARRE_CLASS: 'Barre',
  FOOTBALL_AMERICAN: 'American football',
  FOOTBALL_AUSTRALIAN: 'Australian football',
  FRISBEE_PLAYING_GENERAL: 'Frisbee',
  ULTIMATE_FRISBEE: 'Ultimate frisbee',
  VOLLEYBALL_BEACH: 'Beach volleyball',
  PICKELBALL: 'Pickleball',
  CROSS_COUNTRY_SKI: 'Cross-country skiing',
  SNOW_SPORT: 'Snow sports',
  WATER_SPORT: 'Water sports',
  EQUESTRIAN_SPORTS: 'Horse riding',
  JIU_JITSU: 'Jiu-jitsu',
  HIP_HOP: 'Hip-hop dance',
  ELECTRIC_SCOOTER: 'E-scooter',
  FITNESS_GAMING: 'Fitness gaming',
  MUSICAL_PERFORMANCE: 'Music performance',
};

/** "MOUNTAIN_BIKE" → "Mountain bike". */
const humanize = (kind: string) => kind.charAt(0) + kind.slice(1).toLowerCase().replace(/_/g, ' ');

/**
 * The activity type in English. Fitbit's own name (in the account's language)
 * only for activities without a type.
 */
export const exerciseLabel = (e: Exercise) =>
  LABEL[e.kind] ?? (!e.kind || e.kind === 'OTHER' || e.kind === 'EXERCISE_TYPE_UNSPECIFIED' ? e.name || 'Workout' : humanize(e.kind));

/** The activity's round icon in the exercise color (`--s7`); also usable inside an SVG chart. */
export function ExIcon({ e, className, ...rest }: { e: Exercise } & React.SVGProps<SVGSVGElement>) {
  const group = GROUP_OF.get(e.kind) ?? 'other';
  const Line = group in LUCIDE ? LUCIDE[group as keyof typeof LUCIDE] : null;
  return (
    <svg viewBox="-12 -12 24 24" className={cn('shrink-0', className)} aria-hidden {...rest}>
      <circle r={11} fill="var(--s7)" />
      {Line ? (
        <Line x={-6.5} y={-6.5} width={13} height={13} color="#fff" strokeWidth={2.4} />
      ) : (
        <path d={GLYPH[group as Glyph]} transform="translate(-7.5,-7.5) scale(0.625)" fill="#fff" />
      )}
    </svg>
  );
}
