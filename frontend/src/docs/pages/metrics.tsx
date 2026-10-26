// One page per metric: what it is, how it is measured, how to read it, what moves it.
import { A, Callout, Cards, Facts, H2, H3, Swatch, TypicalRange } from '../ui';

export function Metrics() {
  return (
    <>
      <p className="lead">
        Every metric in Fitdata, with what it measures, where its number comes from, typical values and how to interpret changes.
      </p>
      <H2 id="baseline">Compare with yourself</H2>
      <p>
        Most of these metrics vary a lot from person to person: one healthy adult has an HRV of 25 ms, another 90 ms. The ranges on these pages are
        only orientation. What tells you something is a change against <em>your own</em> usual values, which is why Fitdata compares each day with
        your 7-day average and draws 7-day averages on the Trends page.
      </p>
      <H2 id="night">Night</H2>
      <Cards
        items={[
          { to: 'sleep', title: 'Sleep', text: 'Duration, stages, efficiency, restless moments and schedule.' },
          { to: 'resting-heart-rate', title: 'Resting heart rate', text: 'Your heart rate at rest: a key sign of fitness and recovery.' },
          { to: 'hrv', title: 'HRV', text: 'Heart rate variability during sleep: stress and recovery.' },
          { to: 'spo2', title: 'SpO₂', text: 'Blood oxygen saturation while you sleep.' },
          { to: 'breathing-rate', title: 'Breathing rate', text: 'Breaths per minute during sleep.' },
          { to: 'skin-temperature', title: 'Skin temperature', text: 'Nightly wrist temperature and its changes.' },
        ]}
      />
      <H2 id="day">Day</H2>
      <Cards
        items={[
          { to: 'heart-rate', title: 'Heart rate', text: 'Minute by minute through the day and the night.' },
          { to: 'steps', title: 'Steps', text: 'Steps per day and per 15 minutes.' },
          { to: 'zone-minutes', title: 'Zone minutes', text: 'Active Zone Minutes: time in elevated heart rate zones.' },
          { to: 'active-calories', title: 'Active calories', text: 'Energy burned by activity.' },
          { to: 'exercises', title: 'Exercises', text: 'Recorded workouts and their numbers.' },
        ]}
      />
    </>
  );
}

const STAGE_COLORS = {
  deep: 'var(--stage-deep)',
  light: 'var(--stage-light)',
  rem: 'var(--stage-rem)',
  awake: 'var(--stage-awake)',
};

export function Sleep() {
  return (
    <>
      <p className="lead">How long and how well you slept, from the night that ended on each day's morning.</p>
      <Facts
        items={[
          ['Shown', 'Day page: Night section and key numbers; Trends: Sleep stages, Bedtime – wake'],
          ['Unit', 'hours and minutes'],
          ['Source', 'Sleep sessions and stages detected by Fitbit from movement and heart rate'],
        ]}
      />
      <H2 id="duration">Sleep duration (Asleep)</H2>
      <p>
        <strong>Asleep</strong> is the time you actually slept: the whole night minus the time awake in it. It is usually 30–60 minutes shorter than
        the time between falling asleep and waking up, because everyone wakes up briefly several times a night.
      </p>
      <TypicalRange min={4} max={11} band={[7, 9]} unit="h" label="Recommended sleep for adults (18–64 years)" color="var(--stage-light)" />
      <p>
        Health organisations recommend 7–9 hours for adults; 7–8 hours for those over 65. A single short night is normal; what matters is the average
        over a week, which you can see on the Trends page.
      </p>
      <H2 id="stages">Sleep stages</H2>
      <p>
        Through the night you cycle through the stages several times, each cycle taking roughly 90 minutes. Fitbit estimates the stage every 30 seconds
        from your movement and heart rhythm.
      </p>
      <table>
        <thead>
          <tr>
            <th>Stage</th>
            <th>Typical share</th>
            <th>What it is</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              <Swatch color={STAGE_COLORS.light}>Light</Swatch>
            </td>
            <td>50–60 %</td>
            <td>The most common stage; the body relaxes, heart rate and breathing slow down.</td>
          </tr>
          <tr>
            <td>
              <Swatch color={STAGE_COLORS.deep}>Deep</Swatch>
            </td>
            <td>10–25 %</td>
            <td>The most restorative stage for the body: physical recovery, immune system. Mostly in the first half of the night; it declines with age.</td>
          </tr>
          <tr>
            <td>
              <Swatch color={STAGE_COLORS.rem}>REM</Swatch>
            </td>
            <td>20–25 %</td>
            <td>Dreaming; important for memory and mood. Mostly in the second half of the night, so a short night cuts REM first.</td>
          </tr>
          <tr>
            <td>
              <Swatch color={STAGE_COLORS.awake}>Awake</Swatch>
            </td>
            <td>5–15 %</td>
            <td>Short awakenings, most of which you don't remember.</td>
          </tr>
        </tbody>
      </table>
      <p>
        On the Day page, the <strong>Sleep stages</strong> chart shows each stage in its own row, so you see when each happened; the table on the right
        sums them up. On the Trends page, each night is a stacked bar of the four stages.
      </p>
      <H3 id="restless">Restless</H3>
      <p>
        <strong>Restless</strong> moments are very short awakenings within light sleep, as Google Health shows them. They count as light sleep, not as
        time awake; the table shows their total time and how many there were. Many restless moments can come from a warm room, alcohol, a full
        stomach or noise.
      </p>
      <Callout kind="warn" title="Stages are estimates">
        <p>
          A wrist device cannot measure brain waves, which is how a sleep lab tells the stages apart. Fitbit's stages agree reasonably well with a sleep
          lab on average, but single nights can be off. Use the trends more than any single night.
        </p>
      </Callout>
      <H2 id="efficiency">Efficiency</H2>
      <p>
        <strong>Efficiency</strong> is the time asleep as a share of the whole night, from falling asleep to waking up (the "of 7:50" next to it). 85 %
        and more is considered good. Low efficiency means long awakenings during the night.
      </p>
      <H2 id="schedule">Sleep schedule (Bedtime – wake)</H2>
      <p>
        The <strong>Bedtime – wake</strong> row on the Trends page draws each night from falling asleep to waking up. A regular schedule shows as bars
        at the same height. Going to bed and getting up at about the same time every day, weekends included, is one of the most effective things for
        good sleep; large shifts between weekdays and weekends ("social jet lag") show as bars jumping up and down.
      </p>
      <H2 id="which-night">Which sleep counts as the night</H2>
      <p>A night belongs to the day you woke up. For each day, Fitdata takes:</p>
      <ul>
        <li>the first sleep session that ended on that day before 14:00,</li>
        <li>together with any sessions that followed it within 3 hours (Fitbit sometimes splits one night in two when you are awake for a while); the gap between them counts as awake.</li>
      </ul>
      <p>
        Naps and sleep that ended after 14:00 are not part of the night. If you slept through the day after a night shift, that sleep may not show as
        a night.
      </p>
      <H2 id="what-helps">What affects sleep</H2>
      <ul>
        <li>
          <strong>Alcohol</strong> helps you fall asleep but cuts REM, adds awakenings, and raises heart rate through the night.
        </li>
        <li>
          <strong>Late caffeine</strong> (within 6–8 hours of bedtime) shortens deep sleep.
        </li>
        <li>
          <strong>Late, heavy meals and late hard exercise</strong> keep heart rate and temperature up.
        </li>
        <li>
          <strong>A cool, dark, quiet room</strong> and a regular schedule help the most.
        </li>
      </ul>
    </>
  );
}

export function RestingHeartRate() {
  return (
    <>
      <p className="lead">Your heart rate when you are fully at rest. One of the most useful long-term signs of fitness, and a quick sign of strain.</p>
      <Facts
        items={[
          ['Shown', 'Day page: key numbers; Trends: Resting heart rate; Night heart rate chart (when not hovering)'],
          ['Unit', 'beats per minute (bpm)'],
          ['Source', 'Calculated by Google each day from heart rate while you are asleep and still'],
          ['Better', 'lower, compared with your own usual value'],
        ]}
      />
      <TypicalRange
        min={35}
        max={110}
        band={[60, 100]}
        unit="bpm"
        label="Resting heart rate of adults"
        color="var(--s5)"
        marks={[{ at: 50, label: 'endurance athletes' }]}
      />
      <H2 id="reading">How to read it</H2>
      <p>
        For most adults, a resting heart rate between 60 and 100 bpm is normal; well-trained people often have 40–60. More useful than the number
        itself is how it moves:
      </p>
      <ul>
        <li>
          <strong>Slowly going down over weeks or months</strong> usually means better aerobic fitness.
        </li>
        <li>
          <strong>Several beats above your average for a day or more</strong> is a common sign of strain: illness coming on (often a day or two before
          you feel it), alcohol, a hard workout, poor sleep, stress, heat or dehydration.
        </li>
        <li>
          <strong>Going up over weeks</strong> can come from overtraining, long-term stress, less activity or less sleep.
        </li>
      </ul>
      <Callout kind="tip">
        <p>
          Check resting heart rate together with <A to="hrv">HRV</A>: higher resting heart rate with lower HRV on the same morning is a stronger signal
          than either alone.
        </p>
      </Callout>
      <Callout kind="medical">
        <p>A resting heart rate above 100 bpm or below 40 bpm without athletic training, especially with dizziness or shortness of breath, is a reason to see a doctor.</p>
      </Callout>
    </>
  );
}

export function Hrv() {
  return (
    <>
      <p className="lead">
        Heart rate variability: how much the time between heartbeats varies. A higher HRV generally means your body is rested and relaxed, a lower one
        that it is under strain.
      </p>
      <Facts
        items={[
          ['Shown', 'Day page: key numbers and the HRV chart in the night; Trends: HRV'],
          ['Unit', 'milliseconds (ms)'],
          ['Source', 'Calculated by Google from beat-to-beat intervals during sleep, as RMSSD; needs at least 3 hours of sleep'],
          ['Better', 'higher, compared with your own usual value'],
        ]}
      />
      <H2 id="what-it-is">What it is</H2>
      <p>
        Your heart doesn't beat like a metronome. Even at 60 bpm, the gaps between beats vary, say 980 ms, then 1,030 ms, then 995 ms. These changes
        come from the autonomic nervous system: the "rest and digest" part (parasympathetic) makes them bigger, the "fight or flight" part
        (sympathetic) makes them smaller.
      </p>
      <p>
        Fitbit measures HRV while you sleep, when nothing else disturbs it, and reports <strong>RMSSD</strong>: the root mean square of the differences
        between successive beats. The HRV chart on the Day page shows its individual measurements through the night as dots; the number is the
        night's average.
      </p>
      <TypicalRange min={0} max={120} band={[20, 80]} unit="ms" label="Nightly RMSSD of most adults; very individual and lower with age" color="var(--s7)" />
      <H2 id="reading">How to read it</H2>
      <ul>
        <li>
          <strong>Only compare with yourself.</strong> HRV depends heavily on age, genetics and fitness. 30 ms can be excellent for one person and low for
          another.
        </li>
        <li>
          <strong>A drop well below your 7-day average</strong> often follows alcohol, illness, a hard training day, a late meal, stress or a short
          night.
        </li>
        <li>
          <strong>HRV rising over weeks</strong> usually comes with better fitness, sleep and lower stress.
        </li>
        <li>
          <strong>It is noisy.</strong> Night-to-night changes of 10–20 % are normal. Follow the thick 7-day average line on the Trends page rather than
          single nights.
        </li>
      </ul>
      <Callout kind="note" title="No HRV for a night?">
        <p>Fitbit computes HRV only for sleep of at least 3 hours, and needs a steady signal: a loose band or a lot of movement can leave a night without a value.</p>
      </Callout>
    </>
  );
}

export function Spo2() {
  return (
    <>
      <p className="lead">Blood oxygen saturation: how much of the hemoglobin in your blood carries oxygen, measured while you sleep.</p>
      <Facts
        items={[
          ['Shown', 'Day page: key numbers and the SpO₂ chart in the night; Trends: SpO₂'],
          ['Unit', 'percent (%)'],
          ['Source', 'Measured optically at the wrist (red and infrared light); nightly average by Google'],
          ['Better', 'no single direction; stable in the normal range is good'],
        ]}
      />
      <TypicalRange min={85} max={100} band={[95, 100]} unit="%" label="Normal blood oxygen saturation at sea level" color="var(--s1)" marks={[{ at: 90, label: 'low' }]} />
      <H2 id="reading">How to read it</H2>
      <p>
        In healthy people the nightly average is usually between 95 and 98 %, and it barely changes from night to night. Short dips during the night are
        normal and often come from the band moving or your position.
      </p>
      <ul>
        <li>
          <strong>Altitude</strong> lowers it: in the mountains a few percent less is normal.
        </li>
        <li>
          <strong>A lower average than usual for several nights</strong>, or many deep dips, can come with a respiratory infection or breathing problems
          during sleep (such as sleep apnea, often together with snoring).
        </li>
      </ul>
      <Callout kind="medical">
        <p>
          A wrist sensor is less precise than a fingertip pulse oximeter, especially below 90 %. If you see repeatedly low values, feel unusually tired
          during the day, or someone has noticed pauses in your breathing at night, talk to a doctor.
        </p>
      </Callout>
    </>
  );
}

export function BreathingRate() {
  return (
    <>
      <p className="lead">The average number of breaths per minute during sleep.</p>
      <Facts
        items={[
          ['Shown', 'Day page: key numbers; Trends: Breathing rate'],
          ['Unit', 'breaths per minute'],
          ['Source', 'Calculated by Google from heart rhythm during sleep; needs at least 3 hours of sleep'],
          ['Better', 'no single direction; stable is good'],
        ]}
      />
      <TypicalRange min={8} max={24} band={[12, 20]} unit="/min" label="Breathing rate of adults at rest" color="var(--s3)" />
      <H2 id="reading">How to read it</H2>
      <p>
        Breathing at rest is usually between 12 and 20 breaths a minute, often at the lower end during sleep. Your own value is very stable from night
        to night, which makes changes easy to see.
      </p>
      <ul>
        <li>
          <strong>A rise of one or more breaths per minute above your usual value</strong>, especially together with a higher resting heart rate and
          skin temperature, often comes with an infection, sometimes before you feel ill.
        </li>
        <li>Alcohol, a warm room and hard training the day before can raise it slightly too.</li>
      </ul>
    </>
  );
}

export function SkinTemperature() {
  return (
    <>
      <p className="lead">
        The temperature of the skin on your wrist during sleep. Its absolute value says little; changes against your usual nights say a lot.
      </p>
      <Facts
        items={[
          ['Shown', 'Day page: Skin temperature in the night summary; Trends: Skin temp. change'],
          ['Unit', '°C or °F (Settings → Units)'],
          ['Source', 'Wrist sensor, nightly value calculated by Google'],
          ['Better', 'no single direction; close to your average is usual'],
        ]}
      />
      <H2 id="absolute">The night's value</H2>
      <p>
        The Day page shows the night's skin temperature, typically around 32–35 °C (90–95 °F). That is several degrees below your core body
        temperature (about 37 °C) and depends on the room, the blanket and how the band sits, so it is not a fever thermometer.
      </p>
      <H2 id="change">Change from your average</H2>
      <p>
        The Trends page therefore shows each night's <strong>difference from the average of all your nights</strong>:{' '}
        <Swatch color="var(--s2)">orange</Swatch> bars above zero for warmer nights, <Swatch color="var(--s1)">blue</Swatch> bars below zero for cooler
        ones. Differences within about ±0.5 °C are common.
      </p>
      <ul>
        <li>
          <strong>Clearly warmer for one or more nights</strong>: fever or infection coming on, alcohol, a late hard workout, a warmer bedroom or a
          heavier blanket.
        </li>
        <li>
          <strong>A regular monthly pattern</strong>: in people with a menstrual cycle, temperature is usually 0.2–0.5 °C higher in the second half of
          the cycle (after ovulation) and drops around the period.
        </li>
        <li>
          <strong>Cooler nights</strong>: often just a colder room or the band sitting looser.
        </li>
      </ul>
      <Callout kind="tip">
        <p>
          A warmer night together with a higher <A to="resting-heart-rate">resting heart rate</A> and <A to="breathing-rate">breathing rate</A> is one of
          the more reliable early signs that you might be getting ill.
        </p>
      </Callout>
    </>
  );
}

export function HeartRate() {
  return (
    <>
      <p className="lead">Your heart rate through the day and the night, minute by minute.</p>
      <Facts
        items={[
          ['Shown', 'Day page: Heart rate charts in the Night and Day sections; lowest, average, highest'],
          ['Unit', 'beats per minute (bpm)'],
          ['Source', 'Optical sensor of the device, readings every few seconds'],
        ]}
      />
      <H2 id="day">During the day</H2>
      <p>
        The heart rate chart in the Day section shows, for each minute, the <strong>average</strong> as a line and the <strong>lowest and highest</strong>{' '}
        reading as a light band around it. A narrow band means a steady heart rate; a wide one that it changed quickly, as when you start running or
        climb stairs.
      </p>
      <p>
        Recorded exercises are shaded and marked with an icon above the chart, so you can see how hard each one was. Hovering the chart shows the
        activity at that moment.
      </p>
      <H3 id="day-range">Lowest, average, highest</H3>
      <p>
        Next to the chart are the day's <strong>lowest</strong>, <strong>average</strong> and <strong>highest</strong> heart rate, from every reading of
        the calendar day, night included. The lowest is usually during sleep; the highest during exercise. The average depends mostly on how much you
        moved.
      </p>
      <H2 id="night">At night</H2>
      <p>
        The night's heart rate chart shows the average per minute while you slept. In a good night it falls during the first hours and is lowest in
        deep sleep, then rises slightly toward the morning, with short peaks in REM sleep and when you wake up.
      </p>
      <p>
        If it stays high for most of the night, or falls only late, the body was busy with something: alcohol, a late meal, a late workout, illness or
        stress. The lowest point coming late in the night is a typical sign of that.
      </p>
      <H3 id="asleep">Heart rate asleep</H3>
      <p>
        <strong>Heart rate asleep</strong> in the night summary is Google's average heart rate during non-REM (light and deep) sleep. It is usually
        close to your <A to="resting-heart-rate">resting heart rate</A>, a little above or below it.
      </p>
      <H2 id="zones">Heart rate zones</H2>
      <p>
        How hard an effort was is described by heart rate zones, relative to your heart rate reserve (the range between your resting and maximum heart
        rate). Fitbit uses them for <A to="zone-minutes">Active Zone Minutes</A>.
      </p>
    </>
  );
}

export function Steps() {
  return (
    <>
      <p className="lead">Steps counted by the device's motion sensor.</p>
      <Facts
        items={[
          ['Shown', 'Day page: key numbers and Steps per 15 min; Trends: Steps and the All days table'],
          ['Unit', 'steps'],
          ['Source', 'Accelerometer of the device; Fitdata sums them per 15 minutes and per day'],
          ['Better', 'more, up to a point'],
        ]}
      />
      <H2 id="per-15-min">Steps per 15 minutes</H2>
      <p>
        On the Day page, each bar is the number of steps in a quarter of an hour. It shows when in the day you moved and how long you sat still. A
        brisk walk is about 1,500–1,800 steps in 15 minutes, a run more.
      </p>
      <H2 id="goal">The 10,000 goal</H2>
      <p>
        The dashed line on the Trends page marks 10,000 steps, the goal most step counters use. Research shows that health benefits grow quickly up to
        about 7,000–8,000 steps a day and level off around 8,000–10,000 (lower for people over 60). Any increase from a low starting point counts.
      </p>
      <TypicalRange
        min={0}
        max={15000}
        band={[7000, 10000]}
        unit="steps"
        label="Steps a day where most of the health benefits are reached"
        color="var(--s3)"
        marks={[{ at: 4000, label: 'mostly sitting' }]}
      />
      <Callout kind="note">
        <p>Cycling, swimming and strength training produce few or no steps, even though they are real exercise. Look at <A to="zone-minutes">zone minutes</A> for those.</p>
      </Callout>
    </>
  );
}

export function ZoneMinutes() {
  return (
    <>
      <p className="lead">Active Zone Minutes (AZM): minutes when your heart rate was high enough to count as moderate or vigorous activity.</p>
      <Facts
        items={[
          ['Shown', 'Day page: key numbers and each exercise; Trends: Zone minutes and the All days table (AZM)'],
          ['Unit', 'minutes'],
          ['Source', 'Calculated by Fitbit from your heart rate zones; Fitdata sums them per day'],
          ['Better', 'more, toward 150 a week'],
        ]}
      />
      <H2 id="how-counted">How they are counted</H2>
      <p>
        Fitbit sets your heart rate zones from your age and resting heart rate, as shares of your heart rate reserve (the range between resting and
        maximum heart rate):
      </p>
      <table>
        <thead>
          <tr>
            <th>Zone</th>
            <th>Heart rate reserve</th>
            <th>Feels like</th>
            <th>Counts</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Fat burn</td>
            <td>40–59 %</td>
            <td>brisk walk, you can still talk</td>
            <td>1 per minute</td>
          </tr>
          <tr>
            <td>Cardio</td>
            <td>60–84 %</td>
            <td>running, hard cycling</td>
            <td>2 per minute</td>
          </tr>
          <tr>
            <td>Peak</td>
            <td>85 % and more</td>
            <td>intervals, sprints</td>
            <td>2 per minute</td>
          </tr>
        </tbody>
      </table>
      <p>So 30 minutes of running in the cardio zone give about 60 zone minutes, while 30 minutes of a brisk walk give about 30.</p>
      <H2 id="goal">The weekly goal</H2>
      <p>
        The World Health Organization recommends at least 150 minutes of moderate activity a week, or 75 minutes of vigorous activity. Fitbit's default
        goal is 150 zone minutes a week, and since vigorous minutes count twice, both recommendations meet it. The dashed line on the Trends page marks
        22 minutes a day, which adds up to 150 a week; the week counts, not every single day.
      </p>
    </>
  );
}

export function ActiveCalories() {
  return (
    <>
      <p className="lead">The energy you burned through activity, on top of what your body burns at rest.</p>
      <Facts
        items={[
          ['Shown', 'Day page: key numbers and each exercise; Trends: Active kcal'],
          ['Unit', 'kilocalories (kcal)'],
          ['Source', 'Estimated by Fitbit from heart rate, movement and your profile (age, sex, height, weight); Fitdata sums them per day'],
        ]}
      />
      <H2 id="reading">How to read it</H2>
      <p>
        Your body burns energy all the time just to keep running: the basal metabolic rate, typically 1,300–1,900 kcal a day for adults. Active
        calories are only what comes on top of that through movement, from walking around the flat to a long run. Total energy use is the two
        together.
      </p>
      <p>
        A few hundred active kcal is usual on a quiet day; a long run or ride can add 500–1,000 more. Because they depend on your heart rate, active
        calories reflect both how much and how hard you moved.
      </p>
      <Callout kind="warn">
        <p>
          Calorie estimates from wrist devices can be off by 20 % or more. They are good for comparing your own days with each other, less so for
          counting what to eat.
        </p>
      </Callout>
    </>
  );
}

export function Exercises() {
  return (
    <>
      <p className="lead">Workouts recorded on your device or detected automatically by Fitbit.</p>
      <Facts
        items={[
          ['Shown', 'Day page: icons and shaded bands in the Day charts, list next to them'],
          ['Source', 'Exercise sessions from Fitbit, with Fitbit’s summary of each'],
        ]}
      />
      <H2 id="in-the-charts">In the charts</H2>
      <p>
        Each exercise appears as a round icon above the heart rate chart, and as a faint shaded band over its time in the heart rate and steps charts.
        Hovering a band, or the exercise in the list, highlights it.
      </p>
      <H2 id="numbers">The numbers</H2>
      <p>For each exercise the list shows:</p>
      <ul>
        <li>
          <strong>Duration</strong>: the active time; pauses don't count, so it can be shorter than start to end.
        </li>
        <li>
          <strong>Average heart rate</strong> during the exercise.
        </li>
        <li>
          <strong>kcal</strong>: calories burned during it.
        </li>
        <li>
          <strong>AZM</strong>: the <A to="zone-minutes">zone minutes</A> it earned.
        </li>
      </ul>
      <p>
        Running, walking, hiking, cycling and swimming have their own icons and English names; other activities are shown with Fitbit's name for them,
        which may be in the language of your Fitbit account.
      </p>
      <Callout kind="note" title="Missing an exercise?">
        <p>
          Fitbit detects walks, runs and rides automatically only after about 15 minutes of continuous activity. Shorter or other workouts appear only
          if you start them on the device or log them in the Fitbit app.
        </p>
      </Callout>
    </>
  );
}
