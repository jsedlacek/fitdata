// Ask AI, settings, data and the FAQ.
import { A, Callout, H2, H3 } from '../ui';

export function AskAi() {
  return (
    <>
      <p className="lead">
        Connect Claude or ChatGPT to Fitdata and ask about your data in plain words: "Why am I so tired today?", "Do I sleep better after evening
        runs?"
      </p>
      <p>
        Fitdata runs an MCP server (Model Context Protocol, the standard way to give AI assistants access to data). The assistant asks Fitdata for the
        data it needs to answer you; access is read-only and limited to your own data.
      </p>
      <H2 id="claude">Claude</H2>
      <ol>
        <li>
          In Claude, open <strong>Settings → Connectors</strong> and choose <strong>Add custom connector</strong>.
        </li>
        <li>
          Paste the address <code>https://fitdata.jakub.contact/mcp</code> (also under <strong>Ask Claude or ChatGPT</strong> in the dashboard's header).
        </li>
        <li>Sign in with the same Google account you use for Fitdata.</li>
      </ol>
      <H2 id="chatgpt">ChatGPT</H2>
      <ol>
        <li>
          In ChatGPT, open <strong>Settings → Apps & Connectors → Advanced</strong> and turn on <strong>Developer mode</strong>.
        </li>
        <li>
          Choose <strong>Create</strong>, paste <code>https://fitdata.jakub.contact/mcp</code> and choose <strong>OAuth</strong>.
        </li>
        <li>Sign in with the same Google account you use for Fitdata.</li>
      </ol>
      <H2 id="examples">What to ask</H2>
      <ul>
        <li>I feel off. Am I getting sick?</li>
        <li>How did I sleep this week compared with last month?</li>
        <li>Do I sleep better when I go to bed before 23:00?</li>
        <li>Do evening workouts change my deep sleep or HRV?</li>
        <li>Is my resting heart rate going down since I started running?</li>
        <li>Compare my weekdays and weekends.</li>
      </ul>
      <Callout kind="tip">
        <p>Ask the assistant to show you the numbers it used. It can also draw charts of your data.</p>
      </Callout>
      <H2 id="tools">What the assistant can read</H2>
      <p>The assistant gets these tools:</p>
      <ul>
        <li>
          <code>get_daily_metrics</code>: one row per day with every daily metric, the same ones as on the Trends page.
        </li>
        <li>
          <code>get_day</code>: one day in detail: heart rate, steps, the night's stages, exercises.
        </li>
        <li>
          <code>get_sleep</code> and <code>get_exercises</code>: nights and workouts over a period.
        </li>
        <li>
          <code>get_schema</code> and <code>query</code>: read-only SQL over your tables, for anything else.
        </li>
      </ul>
      <p>
        Dates and times are in your time zone, temperatures in your units, and weeks start on your chosen day (see <A to="settings">Settings</A>).
      </p>
      <Callout kind="medical">
        <p>AI assistants can misread data and sound more certain than they should. Treat their answers as ideas, not diagnoses.</p>
      </Callout>
    </>
  );
}

export function SettingsDoc() {
  return (
    <>
      <p className="lead">The gear icon in the top right opens Settings.</p>
      <H2 id="time-zone">Time zone</H2>
      <p>
        Days start and end at midnight in this time zone, and all times are shown in it. It is set from your browser when you first sign in. If you
        move, change it here: Fitdata recalculates all your days in the new time zone, which takes a few minutes.
      </p>
      <H2 id="units">Units</H2>
      <p>Metric (°C, km) or imperial (°F, mi). Used for temperatures in the dashboard and for every value Claude or ChatGPT tell you.</p>
      <H2 id="week-start">Week starts on</H2>
      <p>Monday, Sunday or Saturday: for the calendar, weekly ticks on the Trends axis, and weekly summaries from the AI.</p>
      <H2 id="clock">Time format</H2>
      <p>24-hour (18:30) or 12-hour (6:30 pm).</p>
      <H2 id="theme">Theme</H2>
      <p>Light, dark, or following your system. Saved in the browser, not in your account.</p>
      <H2 id="export">Download your data</H2>
      <p>
        <strong>Your data → Download</strong> gives a ZIP of everything Fitdata stores about you: the raw data from Google and the tables made from it.
      </p>
      <H2 id="delete">Delete account</H2>
      <p>
        Deletes your account and all downloaded data, and revokes Fitdata's access to your Google Health data. It cannot be undone. Your data in Fitbit
        and Google is not affected.
      </p>
      <H2 id="support">Help and your user ID</H2>
      <p>
        The <strong>Help</strong> card has the contact address and your user ID. Include the ID when you write about a problem, so it can be found
        without your e-mail address being connected to your data.
      </p>
    </>
  );
}

export function DataPrivacy() {
  return (
    <>
      <p className="lead">What Fitdata stores, where, and who can see it.</p>
      <H2 id="what">What is stored</H2>
      <ul>
        <li>The raw data from the Google Health API, as downloaded, and the tables made from it.</li>
        <li>Your settings.</li>
        <li>The access token for Google Health, encrypted with a key in Google Cloud KMS.</li>
      </ul>
      <p>
        Fitdata doesn't ask for your name or e-mail address and doesn't know them; your account is the anonymous Google Health user ID.
      </p>
      <H2 id="where">Where</H2>
      <p>On Google Cloud in Belgium (europe-west1), encrypted. Each user's data is kept separately, and only you can see yours.</p>
      <H2 id="who">Who can see it</H2>
      <p>
        Only you, in the dashboard, and the AI assistants you connect yourself. The data is not sold, shared or used to train AI. See the{' '}
        <A to="/privacy">privacy policy</A> for the details.
      </p>
      <H2 id="history">How far back</H2>
      <p>Your whole history from the Google Health API, from the first day your Fitbit recorded something, and every day from then on.</p>
    </>
  );
}

export function Faq() {
  return (
    <>
      <H2 id="no-data-today">Today's data is missing or old</H2>
      <p>
        Fitdata sees only what has reached Google. Open the Fitbit app so the watch syncs with the phone, then press the sync button in the header.
        Sleep usually appears some time after you wake up, once Fitbit has processed it.
      </p>
      <H2 id="missing-metric">A metric is missing</H2>
      <ul>
        <li>
          <strong>Your device doesn't measure it.</strong> Not every Fitbit has sensors for SpO₂ or skin temperature.
        </li>
        <li>
          <strong>The night was too short.</strong> HRV, breathing rate and SpO₂ need at least 3 hours of sleep.
        </li>
        <li>
          <strong>The watch didn't get a good signal</strong>, for example because the band was loose.
        </li>
        <li>
          <strong>Skin temperature needs a few nights</strong> before Fitbit reports it.
        </li>
      </ul>
      <H2 id="different-from-fitbit">The numbers differ from the Fitbit app</H2>
      <p>Usually for one of these reasons:</p>
      <ul>
        <li>
          Fitdata hasn't synced the latest data yet; press the sync button.
        </li>
        <li>
          A different day: in Fitdata a night belongs to the day you woke up (see <A to="sleep#which-night">which sleep counts as the night</A>), and
          days follow the time zone in <A to="settings#time-zone">Settings</A>.
        </li>
        <li>
          Fitdata merges a night that Fitbit split into several sessions, and doesn't count naps.
        </li>
        <li>Sums like steps and calories are added up from the detailed data and can differ by a few from the app's rounding.</li>
      </ul>
      <H2 id="sync-failed">The sync failed</H2>
      <p>
        An occasional failed sync is usually a temporary problem at Google; the next hourly sync catches up. If a yellow banner says the sync lost
        access, click <strong>Restore access to data</strong> and allow access again.
      </p>
      <H2 id="no-night">A night is missing</H2>
      <p>
        If Fitbit didn't record the sleep, or it ended after 14:00 (for example after a night shift), there is no night for that day. See{' '}
        <A to="sleep#which-night">which sleep counts as the night</A>.
      </p>
      <H2 id="other-people">Can others see my data?</H2>
      <p>
        No. Links to days only work for the signed-in user, and each person sees only their own data. See <A to="data">Your data</A>.
      </p>
      <H2 id="contact">Something else</H2>
      <H3 id="write">Write to us</H3>
      <p>
        Questions, problems and ideas: <A to="mailto:fitdata@jakub.contact">fitdata@jakub.contact</A>. Fitdata is open source; you can also open an
        issue on <A to="https://github.com/jsedlacek/fitdata">GitHub</A>.
      </p>
    </>
  );
}
