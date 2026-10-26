// The docs' pages in reading order, grouped as in the sidebar.
import type { ComponentType } from 'react';
import { Charts, DayPage, GettingStarted, Introduction, TrendsPage } from './basics';
import {
  ActiveCalories,
  BreathingRate,
  Exercises,
  HeartRate,
  Hrv,
  Metrics,
  RestingHeartRate,
  SkinTemperature,
  Sleep,
  Spo2,
  Steps,
  ZoneMinutes,
} from './metrics';
import { AskAi, DataPrivacy, Faq, SettingsDoc } from './more';

export type Page = {
  slug: string;
  title: string;
  group: string;
  /** One sentence for search results and the page's meta description. */
  summary: string;
  /** Other words people search for, e.g. abbreviations. */
  keywords?: string;
  Body: ComponentType;
};

export const PAGES: Page[] = [
  { slug: '', title: 'Introduction', group: 'Getting started', summary: 'What Fitdata is and how it works.', keywords: 'overview about', Body: Introduction },
  { slug: 'getting-started', title: 'Getting started', group: 'Getting started', summary: 'Sign in, connect Fitbit, first and hourly sync.', keywords: 'sign in login connect sync setup', Body: GettingStarted },
  { slug: 'day', title: 'The Day page', group: 'Dashboard', summary: 'One night and day in detail, and the change against your average.', keywords: 'daily overview kpi', Body: DayPage },
  { slug: 'trends', title: 'The Trends page', group: 'Dashboard', summary: 'Every metric day by day over weeks and months.', keywords: 'history chart average weekly monthly', Body: TrendsPage },
  { slug: 'charts', title: 'Reading the charts', group: 'Dashboard', summary: 'Cursor, keyboard, zoom, colors and gaps.', keywords: 'graph hover zoom keyboard shortcuts legend', Body: Charts },
  { slug: 'metrics', title: 'Overview', group: 'Metrics', summary: 'All metrics, and why to compare with yourself.', keywords: 'baseline', Body: Metrics },
  { slug: 'sleep', title: 'Sleep', group: 'Metrics', summary: 'Duration, stages, efficiency, restless moments and schedule.', keywords: 'deep light rem awake bedtime wake efficiency restless nap', Body: Sleep },
  { slug: 'resting-heart-rate', title: 'Resting heart rate', group: 'Metrics', summary: 'Heart rate at rest: fitness and recovery.', keywords: 'rhr pulse bpm', Body: RestingHeartRate },
  { slug: 'hrv', title: 'HRV', group: 'Metrics', summary: 'Heart rate variability during sleep.', keywords: 'heart rate variability rmssd stress recovery', Body: Hrv },
  { slug: 'spo2', title: 'SpO₂', group: 'Metrics', summary: 'Blood oxygen saturation during sleep.', keywords: 'spo2 oxygen saturation pulse oximetry apnea', Body: Spo2 },
  { slug: 'breathing-rate', title: 'Breathing rate', group: 'Metrics', summary: 'Breaths per minute during sleep.', keywords: 'respiratory rate respiration', Body: BreathingRate },
  { slug: 'skin-temperature', title: 'Skin temperature', group: 'Metrics', summary: 'Nightly wrist temperature and its changes.', keywords: 'temp fever cycle', Body: SkinTemperature },
  { slug: 'heart-rate', title: 'Heart rate', group: 'Metrics', summary: 'Minute by minute through the day and night.', keywords: 'pulse bpm hr zones', Body: HeartRate },
  { slug: 'steps', title: 'Steps', group: 'Metrics', summary: 'Steps per day and per 15 minutes.', keywords: 'walking 10000 goal', Body: Steps },
  { slug: 'zone-minutes', title: 'Zone minutes', group: 'Metrics', summary: 'Active Zone Minutes and the weekly goal.', keywords: 'azm active zone minutes fat burn cardio peak who 150', Body: ZoneMinutes },
  { slug: 'active-calories', title: 'Active calories', group: 'Metrics', summary: 'Energy burned by activity.', keywords: 'kcal energy calories bmr', Body: ActiveCalories },
  { slug: 'exercises', title: 'Exercises', group: 'Metrics', summary: 'Recorded workouts and their numbers.', keywords: 'workout activity run ride swim', Body: Exercises },
  { slug: 'ask-ai', title: 'Ask Claude or ChatGPT', group: 'More', summary: 'Connect an AI assistant to your data through MCP.', keywords: 'ai mcp claude chatgpt connector', Body: AskAi },
  { slug: 'settings', title: 'Settings', group: 'More', summary: 'Time zone, units, week start, export and deleting the account.', keywords: 'preferences timezone units export delete theme dark', Body: SettingsDoc },
  { slug: 'data', title: 'Your data', group: 'More', summary: 'What is stored, where, and who can see it.', keywords: 'privacy gdpr security storage', Body: DataPrivacy },
  { slug: 'faq', title: 'FAQ & troubleshooting', group: 'More', summary: 'Missing data, failed syncs and other questions.', keywords: 'help problem missing error support', Body: Faq },
];

export const GROUPS = [...new Set(PAGES.map((p) => p.group))];
