// Getting started and the dashboard pages.
import dayDark from '@/home/day-dark.jpg';
import dayLight from '@/home/day-light.jpg';
import trendsDark from '@/home/trends-dark.jpg';
import trendsLight from '@/home/trends-light.jpg';
import { A, Callout, Cards, H2, H3, Kbd, Shot, Swatch } from '../ui';

export function Introduction() {
  return (
    <>
      <p className="lead">
        Fitdata downloads your Fitbit data through the Google Health API every hour, keeps your whole history, and shows it on two pages: one day in
        detail and trends over weeks and months. You can also connect Claude or ChatGPT and simply ask about your data.
      </p>
      <p>
        This manual explains how Fitdata works and, above all, what each number and chart means and how to read it. Press <Kbd>⌘ K</Kbd> (or{' '}
        <Kbd>Ctrl K</Kbd>) anywhere to search it.
      </p>
      <Cards
        items={[
          { to: 'getting-started', title: 'Getting started', text: 'Sign in, connect Fitbit and what happens during the first sync.' },
          { to: 'day', title: 'The Day page', text: 'Last night and today on one time axis, and how to read it.' },
          { to: 'trends', title: 'The Trends page', text: 'Weeks and months at a glance: averages, baselines and goals.' },
          { to: 'metrics', title: 'Metrics', text: 'What every metric measures, typical values and what moves them.' },
          { to: 'ask-ai', title: 'Ask Claude or ChatGPT', text: 'Connect an AI assistant to your data and ask in plain words.' },
          { to: 'faq', title: 'FAQ & troubleshooting', text: 'Missing data, failed syncs and other common questions.' },
        ]}
      />
      <H2 id="how-it-works">How it works</H2>
      <ol>
        <li>
          Your Fitbit device syncs to the Fitbit app on your phone, which uploads the data to <strong>Google Health</strong> (Fitbit is part of Google).
        </li>
        <li>
          Once an hour Fitdata asks the <strong>Google Health API</strong> for anything new, with the read-only access you granted. You can also start a
          sync yourself with the sync button in the top right.
        </li>
        <li>
          The raw data is stored for your account only, and turned into tables: one row per day, every heart rate reading, each night's sleep stages,
          each exercise.
        </li>
        <li>The dashboard, and Claude or ChatGPT if you connect them, read those tables.</li>
      </ol>
      <p>
        Most of the numbers are Google's own calculations (resting heart rate, HRV, SpO₂, breathing rate, skin temperature, sleep stages, zone
        minutes). Fitdata adds the daily sums and ranges, the averages and comparisons, and the charts. Each metric page says which is which.
      </p>
      <Callout kind="medical">
        <p>
          Fitdata is not a medical device. Wrist sensors estimate these values and can be wrong, especially during movement or with a loose band. Use the
          numbers to notice patterns in your own data, not to diagnose anything. If something worries you, talk to a doctor.
        </p>
      </Callout>
    </>
  );
}

export function GettingStarted() {
  return (
    <>
      <p className="lead">From signing in to a full dashboard takes a few minutes.</p>
      <H2 id="sign-in">1. Sign in with Google</H2>
      <p>
        Use the Google account your Fitbit is linked to. Fitdata asks for no name or e-mail; your account is identified only by the anonymous Google
        Health user ID.
      </p>
      <H2 id="connect">2. Connect your Fitbit</H2>
      <p>
        After signing in, the dashboard shows a <strong>Connect Fitbit</strong> banner. It asks Google for read-only access to your health data: Fitdata
        can read it, never change or delete it.
      </p>
      <H2 id="first-sync">3. The first sync</H2>
      <p>
        Right after connecting, Fitdata downloads your whole history. A progress card shows how far it is: first the last few days, then each data type
        back to the start of your history, and finally the charts are built. It usually takes a few minutes; you can close the page and come back.
      </p>
      <H2 id="hourly-sync">Hourly sync</H2>
      <p>
        From then on a sync runs every hour and downloads the last few days again, so late uploads from your phone are picked up. The button in the
        header shows how the last sync went:
      </p>
      <ul>
        <li>
          <strong>Synced · 12 min ago</strong> with a green check: all good.
        </li>
        <li>
          <strong>Sync failed</strong> with a warning sign: the last run had an error; the next one usually fixes it.
        </li>
        <li>
          <strong>Syncing…</strong>: a sync is running. When it ends, the page reloads its data by itself.
        </li>
      </ul>
      <p>Click the button to sync right away, for example after syncing your watch with the phone.</p>
      <Callout kind="tip" title="Data seems late?">
        <p>
          Fitdata can only see what has reached Google. Open the Fitbit app on your phone so the watch syncs, wait a minute, then press the sync button.
          Sleep in particular is often processed only some time after you wake up.
        </p>
      </Callout>
      <H2 id="lost-access">If access is lost</H2>
      <p>
        If Google stops accepting Fitdata's access (for example after you changed your password or removed access in your Google account), a yellow
        banner offers <strong>Restore access to data</strong>. One click and a new consent, and a sync fills in the missing days.
      </p>
    </>
  );
}

export function DayPage() {
  return (
    <>
      <p className="lead">
        The Day page shows one day: the night that ended that morning and the day that followed, each on its own time axis. It opens on the latest day
        with data.
      </p>
      <Shot light={dayLight} dark={dayDark} alt="The Day page: key numbers at the top, then the night with sleep stages, heart rate, SpO₂ and HRV" />
      <H2 id="choosing-a-day">Choosing a day</H2>
      <ul>
        <li>
          The arrows next to the date go to the previous or next day <em>with data</em>; days without any data are skipped. The keys <Kbd>←</Kbd> and{' '}
          <Kbd>→</Kbd> do the same.
        </li>
        <li>Click the date to open a calendar; days with data are marked.</li>
        <li>
          Every day has its own address (<code>/?date=2026-10-24</code>), so you can bookmark or share a link to it (others still see only their own
          data).
        </li>
      </ul>
      <H2 id="key-numbers">Key numbers</H2>
      <p>
        At the top are the day's main numbers in two groups, <strong>Night</strong> (<A to="sleep">asleep</A>,{' '}
        <A to="resting-heart-rate">resting heart rate</A>, <A to="hrv">HRV</A>, <A to="spo2">SpO₂</A>, <A to="breathing-rate">breathing rate</A>) and{' '}
        <strong>Day</strong> (<A to="steps">steps</A>, <A to="zone-minutes">zone minutes</A>, <A to="active-calories">active calories</A>).
      </p>
      <H3 id="change-vs-average">The change under each number</H3>
      <p>
        The small number below each value compares the day with the <strong>average of the 7 previous days with data</strong>. For example, a resting
        heart rate of 58 with <code>↑ +3</code> means three beats per minute above your recent average.
      </p>
      <ul>
        <li>
          <span className="text-good">Green</span> means better than your average and <span className="text-critical">red</span> worse, but only for
          metrics where one direction is clearly better: more sleep, lower resting heart rate, higher HRV, more steps and more zone minutes.
        </li>
        <li>SpO₂, breathing rate and active calories stay gray: neither direction is simply good.</li>
        <li>
          Changes within 5 % of the average stay gray too; day-to-day noise is that big. <code>= avg</code> means the difference rounds to nothing.
        </li>
      </ul>
      <Callout kind="tip">
        <p>
          One red number means little. Look for several moving together, like a higher resting heart rate, lower HRV and higher breathing rate on the
          same morning, which often comes with illness, alcohol, a hard workout or a short night.
        </p>
      </Callout>
      <H2 id="night">Night</H2>
      <p>
        The night section covers the sleep that ended on this day's morning, with ten minutes on each side. All charts share one time axis: hover any
        of them and a cursor appears in all of them at the same moment, with the values in the chart headers (see{' '}
        <A to="charts">Reading the charts</A>).
      </p>
      <ul>
        <li>
          <strong>Sleep stages</strong>: when you were in deep, light, REM sleep or awake, and restless moments. See <A to="sleep#stages">Sleep stages</A>
          .
        </li>
        <li>
          <strong>Heart rate</strong>: your average heart rate per minute through the night. It normally falls in the first half of the night and is
          lowest in deep sleep. See <A to="heart-rate#night">Heart rate at night</A>.
        </li>
        <li>
          <strong>SpO₂</strong>: blood oxygen saturation, measured repeatedly while you sleep. See <A to="spo2">SpO₂</A>.
        </li>
        <li>
          <strong>HRV</strong>: heart rate variability, each dot one measurement during sleep. See <A to="hrv">HRV</A>.
        </li>
      </ul>
      <p>On the right is a summary of the night:</p>
      <ul>
        <li>
          <strong>Fell asleep</strong> and <strong>Woke up</strong>: the start and end of the night.
        </li>
        <li>
          <strong>Asleep</strong>: time actually asleep, without the time awake. See <A to="sleep#duration">Sleep duration</A>.
        </li>
        <li>
          <strong>Efficiency</strong>: time asleep as a share of the whole night. See <A to="sleep#efficiency">Efficiency</A>.
        </li>
        <li>
          <strong>Heart rate asleep</strong>: your average heart rate during non-REM sleep. See <A to="heart-rate#asleep">Heart rate asleep</A>.
        </li>
        <li>
          <strong>Skin temperature</strong>: the night's wrist skin temperature. See <A to="skin-temperature">Skin temperature</A>.
        </li>
        <li>The stage table: time and share of the night in each stage, and how many restless moments there were.</li>
      </ul>
      <H2 id="day-section">Day</H2>
      <p>The day section runs from the moment you woke up until midnight.</p>
      <ul>
        <li>
          <strong>Heart rate</strong>: the line is your average heart rate in each minute, the light band around it the lowest and highest reading in
          that minute. A wide band means your heart rate changed quickly, typically during activity. See <A to="heart-rate">Heart rate</A>.
        </li>
        <li>
          <strong>Exercise</strong>: recorded activities appear as icons above the heart rate, with a faint shaded band over their time in both charts.
          The list on the right shows each one's duration, average heart rate, calories and zone minutes; hovering an activity in the list highlights it
          in the charts. See <A to="exercises">Exercises</A>.
        </li>
        <li>
          <strong>Steps per 15 min</strong>: each bar is the steps taken in a quarter of an hour, which shows when in the day you moved. See{' '}
          <A to="steps">Steps</A>.
        </li>
        <li>
          On the right: the day's <strong>lowest</strong>, <strong>average</strong> and <strong>highest</strong> heart rate, from every reading of the
          whole calendar day.
        </li>
      </ul>
      <Callout kind="note" title="Which day does a night belong to?">
        <p>
          A night belongs to the day you woke up. The night from Friday to Saturday is on Saturday's page, together with Saturday's activity. See{' '}
          <A to="sleep#which-night">which sleep counts as the night</A>.
        </p>
      </Callout>
    </>
  );
}

export function TrendsPage() {
  return (
    <>
      <p className="lead">
        The Trends page shows every metric day by day over a period: the last 7, 30 or 90 days, or your whole history. It is where slow changes and
        unusual days stand out.
      </p>
      <Shot light={trendsLight} dark={trendsDark} alt="The Trends page over 90 days: sleep stages, bedtime and wake time, resting heart rate, HRV, SpO₂, breathing rate and skin temperature" />
      <H2 id="period">Period</H2>
      <p>
        Choose the period in the top right. The number next to each row's title is the average over the period (<em>30-day avg</em>), until you hover
        a day; then it shows that day's value.
      </p>
      <H2 id="lines">Lines: daily values and the 7-day average</H2>
      <p>
        Resting heart rate, HRV, SpO₂ and breathing rate are drawn as a <strong>line</strong> with the value of each day (with dots for periods up to 45
        days). From 30 days up a <strong>soft, wide band</strong> of the same color runs behind it: the <strong>7-day average</strong>, each day's value
        averaged with the six days before it (at least three of them with data).
      </p>
      <p>
        Single days jump around for many reasons; the band shows the direction. One odd night barely moves it, and a trend worth noticing is one the
        band follows for a week or more.
      </p>
      <Callout kind="warn" title="The scale is zoomed in">
        <p>
          To make changes visible, these rows don't start at zero: the scale fits the values of the period. A line that looks steep may be a change of
          only 2 bpm or 0.5 %. Always check the numbers on the axis.
        </p>
      </Callout>
      <H2 id="night-rows">Night rows</H2>
      <p>Each night is dated by the morning it ended, like on the Day page.</p>
      <ul>
        <li>
          <strong>Sleep stages</strong>: one stacked bar per night, from the bottom <Swatch color="var(--stage-deep)">Deep</Swatch>,{' '}
          <Swatch color="var(--stage-light)">Light</Swatch>, <Swatch color="var(--stage-rem)">REM</Swatch> and on top{' '}
          <Swatch color="var(--stage-awake)">Awake</Swatch>. The bar's height without the awake part is the time asleep. See{' '}
          <A to="sleep">Sleep</A>.
        </li>
        <li>
          <strong>Bedtime – wake</strong>: one bar per night from falling asleep (top) to waking up (bottom); the axis shows the clock time, earlier is
          higher. Bars at the same height mean a regular schedule. See <A to="sleep#schedule">Sleep schedule</A>.
        </li>
        <li>
          <A to="resting-heart-rate">Resting heart rate</A>, <A to="hrv">HRV</A>, <A to="spo2">SpO₂</A>, <A to="breathing-rate">Breathing rate</A>:
          lines as described above.
        </li>
        <li>
          <strong>Skin temp. change</strong>: bars above zero in <Swatch color="var(--s2)">orange</Swatch> for nights warmer than your average, below
          zero in <Swatch color="var(--s1)">blue</Swatch> for cooler ones. The average is over your whole history, so it doesn't move with the period.
          See <A to="skin-temperature">Skin temperature</A>.
        </li>
      </ul>
      <H2 id="day-rows">Day rows</H2>
      <ul>
        <li>
          <strong>Steps</strong>: one bar per day; the dashed line marks 10,000 steps. See <A to="steps">Steps</A>.
        </li>
        <li>
          <strong>Zone minutes</strong>: Active Zone Minutes per day; the dashed line marks 22 a day, which adds up to the recommended 150 a week. See{' '}
          <A to="zone-minutes">Zone minutes</A>.
        </li>
        <li>
          <strong>Active kcal</strong>: calories burned by activity on top of your resting metabolism. See <A to="active-calories">Active calories</A>.
        </li>
      </ul>
      <H2 id="all-days">All days table</H2>
      <p>
        Under the charts, a table lists the days of the period, newest first: sleep, resting heart rate (RHR), HRV, steps and zone minutes (AZM).
        Hovering a row moves the cursor in the charts to that day; clicking it opens the day on the Day page. <em>Show more</em> lists all days.
      </p>
      <H2 id="zoom">Zooming in</H2>
      <p>
        Drag across any chart to zoom all of them to the selected days. Double-click a chart or click <strong>Reset zoom</strong> under the charts to go
        back. See <A to="charts">Reading the charts</A>.
      </p>
    </>
  );
}

export function Charts() {
  return (
    <>
      <p className="lead">All charts in Fitdata work the same way. A few things are worth knowing.</p>
      <H2 id="cursor">One cursor for all charts</H2>
      <p>
        Charts in one section share the time axis. Hovering any of them shows a vertical line in all of them at the same moment, a dot on each line,
        and the values at that moment in each chart's header. The time or date is shown on the axis below. When you are not hovering, the headers show
        a summary instead: the night's or day's value, or the average of the period.
      </p>
      <H2 id="keyboard">Keyboard</H2>
      <ul>
        <li>
          <Kbd>Tab</Kbd> to a chart, then <Kbd>←</Kbd> and <Kbd>→</Kbd> move the cursor: by 5 minutes on the Day page (<Kbd>Shift</Kbd> for an hour), by a
          day on the Trends page. In charts of separate measurements, like HRV and steps, it jumps from one to the next. <Kbd>Esc</Kbd> hides it.
        </li>
        <li>
          Elsewhere on the Day page, <Kbd>←</Kbd> and <Kbd>→</Kbd> go to the previous and next day.
        </li>
        <li>
          In these docs, <Kbd>⌘ K</Kbd>, <Kbd>Ctrl K</Kbd> or <Kbd>/</Kbd> opens the search.
        </li>
      </ul>
      <H2 id="zoom">Zoom</H2>
      <p>
        On the Trends page, drag across a chart to zoom into the selected days; double-click or <strong>Reset zoom</strong> returns to the whole period.
        The Day page's charts always show the whole night or day.
      </p>
      <H2 id="colors">Colors</H2>
      <p>Each metric has the same color everywhere. Sleep stages use one family of blues for sleep and amber for awake:</p>
      <p className="flex flex-wrap gap-x-5 gap-y-2">
        <Swatch color="var(--stage-deep)">Deep</Swatch>
        <Swatch color="var(--stage-light)">Light</Swatch>
        <Swatch color="var(--stage-rem)">REM</Swatch>
        <Swatch color="color-mix(in srgb, var(--stage-awake) 55%, var(--page))">Restless</Swatch>
        <Swatch color="var(--stage-awake)">Awake</Swatch>
      </p>
      <p>
        Green and red are used only for good and bad (the changes against your average), always with an arrow, so they don't depend on color alone.
      </p>
      <H2 id="gaps">Gaps and missing data</H2>
      <p>
        A line breaks where there is no data, rather than connecting across the gap: for example while you were not wearing the watch or it was
        charging. A dash (–) means no value for that day. Not every device measures everything; see{' '}
        <A to="faq#missing-metric">why a metric is missing</A>.
      </p>
      <H2 id="time">Times and time zones</H2>
      <p>
        Times are shown in the time zone from <A to="settings">Settings</A>, in the 24- or 12-hour clock you chose. On the night the clocks go back,
        the repeated hour appears twice on the axis, with the zone name to tell them apart (<code>02:30 CEST</code>, then <code>02:30 CET</code>).
      </p>
    </>
  );
}
