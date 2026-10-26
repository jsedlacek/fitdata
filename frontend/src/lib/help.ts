// Short explanations behind the dashboard's (?) icons, each with the docs page that says more.

export type Topic = { title: string; text: string; doc: string };

export const HELP = {
  kpis: {
    title: 'Change against your average',
    text: 'The small number compares the day with the average of the 7 previous days with data. Green and red only where higher or lower is clearly better, and only for changes over 5 %.',
    doc: 'day#change-vs-average',
  },
  sleep: {
    title: 'Asleep',
    text: 'Time actually asleep in the night that ended this morning, without the time awake. Adults need 7–9 hours; the weekly average matters more than one night.',
    doc: 'sleep#duration',
  },
  stages: {
    title: 'Sleep stages',
    text: 'Deep sleep restores the body and comes mostly early in the night; REM, important for memory and mood, mostly later. Restless moments are short awakenings counted as light sleep.',
    doc: 'sleep#stages',
  },
  schedule: {
    title: 'Bedtime – wake',
    text: 'Each bar runs from falling asleep (top) to waking up (bottom); earlier is higher. Bars at the same height mean a regular schedule.',
    doc: 'sleep#schedule',
  },
  efficiency: {
    title: 'Efficiency',
    text: 'Time asleep as a share of the whole night, from falling asleep to waking up. 85 % and more is considered good.',
    doc: 'sleep#efficiency',
  },
  restingHr: {
    title: 'Resting heart rate',
    text: 'Your heart rate at full rest, calculated by Google each day. Lower over months usually means better fitness; a few beats above your average often means strain, illness or alcohol.',
    doc: 'resting-heart-rate',
  },
  nightHr: {
    title: 'Heart rate at night',
    text: 'Average heart rate per minute while asleep. In a good night it falls during the first hours and is lowest in deep sleep.',
    doc: 'heart-rate#night',
  },
  hrAsleep: {
    title: 'Heart rate asleep',
    text: 'Average heart rate during non-REM sleep, calculated by Google; usually close to your resting heart rate.',
    doc: 'heart-rate#asleep',
  },
  hrv: {
    title: 'HRV',
    text: 'Heart rate variability during sleep (RMSSD). Higher usually means rested, lower means strain. Very individual: compare only with your own average.',
    doc: 'hrv',
  },
  spo2: {
    title: 'SpO₂',
    text: 'Blood oxygen saturation while you sleep. Usually 95–98 % on average and stable from night to night.',
    doc: 'spo2',
  },
  breathing: {
    title: 'Breathing rate',
    text: 'Average breaths per minute during sleep, usually 12–20. A rise of one or more above your usual value often comes with illness.',
    doc: 'breathing-rate',
  },
  skinTemp: {
    title: 'Skin temperature',
    text: 'Wrist skin temperature during sleep, several degrees below core temperature. Changes against your usual nights matter, not the value itself.',
    doc: 'skin-temperature',
  },
  skinTempChange: {
    title: 'Skin temperature change',
    text: 'Each night against the average of all your nights: orange warmer, blue cooler. Warmer nights can come with fever, alcohol, a warm room or the menstrual cycle.',
    doc: 'skin-temperature#change',
  },
  dayHr: {
    title: 'Heart rate',
    text: 'The line is the average heart rate in each minute, the band the lowest and highest reading in it. Shaded areas are recorded exercises.',
    doc: 'heart-rate#day',
  },
  steps: {
    title: 'Steps',
    text: 'Steps counted by the device. Most of the health benefits are reached at about 7,000–10,000 a day.',
    doc: 'steps',
  },
  azm: {
    title: 'Zone minutes',
    text: 'Active Zone Minutes: each minute in the fat burn zone counts once, in cardio or peak twice. The goal is 150 a week, about 22 a day.',
    doc: 'zone-minutes',
  },
  kcal: {
    title: 'Active calories',
    text: 'Energy burned by activity on top of your resting metabolism, estimated by Fitbit.',
    doc: 'active-calories',
  },
  trendLines: {
    title: 'Reading the lines',
    text: 'The line and its dots are each day’s value. The soft band behind it is the average of the last 7 days, so one odd night barely moves it but a change that lasts shows. The scale fits the period and does not start at zero, so small changes look big.',
    doc: 'trends#lines',
  },
} satisfies Record<string, Topic>;

export type HelpKey = keyof typeof HELP;
