/**
 * The expression language, documented for the in-app help.
 *
 * Kept as data rather than prose in a component, so the help can be searched
 * and every example can be dropped into the editor with one click. Every
 * result below was worked out against `noceur-fsmodel/src/titlefmt.rs` — this
 * is the engine's behaviour, not foobar2000's, where the two differ.
 */

/** The track every example is run against. */
export const SAMPLE: [string, string][] = [
  ["%artist%", "Daft Punk feat. Pharrell Williams"],
  ["%title%", "Get Lucky (Radio Edit)"],
  ["%album%", "Random Access Memories"],
  ["%year%", "2013"],
  ["%track%", "8"],
  ["%bpm%", "116"],
  ["%key%", "6A"],
  ["%filename%", "08 - get lucky"],
  ["%ext%", "flac"],
];

export type HelpEntry = {
  /** What you type. */
  code: string;
  /** One line on what it does. */
  about: string;
  /** A worked example and what it gives for the sample track. */
  example?: string;
  result?: string;
};

export type HelpSection = { title: string; intro?: string; entries: HelpEntry[] };

export const SYNTAX: HelpSection[] = [
  {
    title: "Basics",
    entries: [
      {
        code: "%field%",
        about: "The value of a tag. Case doesn't matter: %ARTIST% is %artist%.",
        example: "%artist% - %title%",
        result: "Daft Punk feat. Pharrell Williams - Get Lucky (Radio Edit)",
      },
      {
        code: "$function(a,b)",
        about: "Runs a function. Arguments are split by commas.",
        example: "$upper(%album%)",
        result: "RANDOM ACCESS MEMORIES",
      },
      {
        code: "[ … ]",
        about: "Optional part: dropped when no field inside it has a value.",
        example: "%title%[ '['%key%']']",
        result: "Get Lucky (Radio Edit) [6A]",
      },
      {
        code: "'text'",
        about:
          "Quoted text is used as is. Quote anything containing % $ [ ] , ( ). Write '' for one quote.",
        example: "%artist% '(%)' %bpm%",
        result: "Daft Punk feat. Pharrell Williams (%) 116",
      },
    ],
  },
  {
    title: "Fields",
    entries: [
      { code: "%artist%", about: "Artist." },
      { code: "%title%", about: "Title." },
      { code: "%album%", about: "Album." },
      { code: "%album artist%", about: "Album artist. %albumartist% works too." },
      { code: "%genre%", about: "Genre." },
      { code: "%year%", about: "Release year. %date% is the same value." },
      { code: "%track%", about: "Track number. %tracknumber% works too." },
      { code: "%bpm%", about: "Tempo, without decimals when it's whole." },
      { code: "%key%", about: "Musical key, as tagged." },
      { code: "%comment%", about: "Comment." },
      { code: "%isrc%", about: "ISRC code." },
      { code: "%filename%", about: "File name without extension." },
      { code: "%filename_ext%", about: "File name with extension." },
      { code: "%ext%", about: "Extension, without the dot." },
      { code: "%directoryname%", about: "Name of the folder the file is in." },
      { code: "%_path%", about: "Full path. %path% works too." },
    ],
  },
  {
    title: "Text",
    entries: [
      { code: "$upper(x)", about: "UPPER CASE.", example: "$upper(%key%)", result: "6A" },
      { code: "$lower(x)", about: "lower case.", example: "$lower(%album%)", result: "random access memories" },
      {
        code: "$caps(x)",
        about: "Capitalises Each Word, lowering the rest.",
        example: "$caps(%filename%)",
        result: "08 - Get Lucky",
      },
      {
        code: "$caps2(x)",
        about: "Capitalises each word, leaves the rest alone (keeps DJ, USA).",
        example: "$caps2(live in USA)",
        result: "Live In USA",
      },
      { code: "$trim(x)", about: "Removes spaces at both ends." },
      {
        code: "$replace(x,a,b,…)",
        about: "Replaces a with b. More pairs can follow. Plain text, no patterns.",
        example: "$replace(%artist%,' feat. ',' & ')",
        result: "Daft Punk & Pharrell Williams",
      },
      { code: "$left(x,n)", about: "First n characters.", example: "$left(%album%,6)", result: "Random" },
      { code: "$right(x,n)", about: "Last n characters.", example: "$right(%album%,8)", result: "Memories" },
      {
        code: "$substr(x,from,to)",
        about: "Characters from…to, counting from 1, both ends included.",
        example: "$substr(%album%,8,13)",
        result: "Access",
      },
      {
        code: "$insert(x,text,n)",
        about: "Puts text after the first n characters.",
        example: "$insert(%key%,' ',1)",
        result: "6 A",
      },
      { code: "$len(x)", about: "Number of characters.", example: "$len(%key%)", result: "2" },
      { code: "$repeat(x,n)", about: "x, n times.", example: "$repeat(-,3)", result: "---" },
      { code: "$pad(x,n)", about: "Adds spaces on the right up to n characters." },
      { code: "$padcut(x,n)", about: "Exactly n characters: cut, or padded with spaces." },
    ],
  },
  {
    title: "Finding text",
    intro: "Positions count from 1. Not found gives 0, which is what $ifgreater(…,0,…) tests.",
    entries: [
      {
        code: "$strchr(x,c)",
        about: "Position of the first c.",
        example: "$strchr(%title%,'(')",
        result: "11",
      },
      { code: "$strrchr(x,c)", about: "Position of the last c." },
      {
        code: "$strstr(x,text)",
        about: "Position of text.",
        example: "$strstr(%artist%,' feat.')",
        result: "10",
      },
    ],
  },
  {
    title: "Numbers",
    intro: "Whole numbers only. Text that isn't a number counts as 0.",
    entries: [
      { code: "$add(a,b,…)", about: "Sum.", example: "$add(%year%,1)", result: "2014" },
      { code: "$sub(a,b,…)", about: "a minus the rest.", example: "$sub(%bpm%,16)", result: "100" },
      { code: "$mul(a,b,…)", about: "Product.", example: "$mul(%bpm%,2)", result: "232" },
      { code: "$div(a,b,…)", about: "Division, rounded down. Dividing by 0 does nothing.", example: "$div(%bpm%,2)", result: "58" },
      { code: "$min(a,b,…)", about: "Smallest." },
      { code: "$max(a,b,…)", about: "Largest." },
      { code: "$num(x)", about: "x as a number: '007' becomes 7." },
      { code: "$abs(x)", about: "Without the minus sign." },
    ],
  },
  {
    title: "Conditions",
    intro:
      "A field counts as present when it has a value. Function results always count as present, so test positions and numbers with $ifgreater or $ifequal, not $if.",
    entries: [
      {
        code: "$if(test,then,else)",
        about: "then when test has a value, else otherwise. else is optional.",
        example: "$if(%bpm%,%bpm% BPM,no BPM)",
        result: "116 BPM",
      },
      {
        code: "$if2(a,b,…)",
        about: "The first one that has a value. $if3 is the same.",
        example: "$if2(%album artist%,%artist%)",
        result: "Daft Punk feat. Pharrell Williams",
      },
      {
        code: "$ifequal(a,b,then,else)",
        about: "Compares two numbers.",
        example: "$ifequal(%track%,1,first,other)",
        result: "other",
      },
      {
        code: "$ifgreater(a,b,then,else)",
        about: "then when a > b (as numbers).",
        example: "$ifgreater(%bpm%,120,fast,slow)",
        result: "slow",
      },
      {
        code: "$iflonger(a,b,then,else)",
        about: "then when a has more characters than b.",
      },
      { code: "$and(a,b,…)", about: "Present when all are present. For use inside $if." },
      { code: "$or(a,b,…)", about: "Present when any is present." },
      { code: "$not(a)", about: "Present when a is not." },
      {
        code: "$meta(name)",
        about: "A field by name, like %name%.",
        example: "$meta(album)",
        result: "Random Access Memories",
      },
    ],
  },
];

/** Complete recipes, ready to use. */
export const RECIPES: { label: string; code: string; result: string }[] = [
  {
    label: "Artist - Title",
    code: "%artist% - %title%",
    result: "Daft Punk feat. Pharrell Williams - Get Lucky (Radio Edit)",
  },
  {
    label: "Two-digit track number",
    code: "$ifgreater(10,%track%,0,)%track% %title%",
    result: "08 Get Lucky (Radio Edit)",
  },
  {
    label: "Title without the bracketed part",
    code: "$ifgreater($strchr(%title%,'('),0,$trim($left(%title%,$sub($strchr(%title%,'('),1))),%title%)",
    result: "Get Lucky",
  },
  {
    label: "Main artist only (before feat.)",
    code: "$ifgreater($strstr(%artist%,' feat.'),0,$left(%artist%,$sub($strstr(%artist%,' feat.'),1)),%artist%)",
    result: "Daft Punk",
  },
  {
    label: "feat. → &",
    code: "$replace(%artist%,' feat. ',' & ',' ft. ',' & ')",
    result: "Daft Punk & Pharrell Williams",
  },
  {
    label: "Key and BPM in front, when tagged",
    code: "['['%key%']' ][%bpm% ]%artist% - %title%",
    result: "[6A] 116 Daft Punk feat. Pharrell Williams - Get Lucky (Radio Edit)",
  },
  {
    label: "Year in brackets, when tagged",
    code: "%album%[ '('%year%')']",
    result: "Random Access Memories (2013)",
  },
  {
    label: "Title Case from a lower-case file name",
    code: "$caps(%filename%)",
    result: "08 - Get Lucky",
  },
  {
    label: "Folder per artist and album (copy / move)",
    code: "%artist%\\%album%\\%filename%",
    result: "Daft Punk feat. Pharrell Williams\\Random Access Memories\\08 - get lucky",
  },
  {
    label: "Pass the file to a program (open)",
    code: '"%_path%"',
    result: '"E:\\music\\08 - get lucky.flac"',
  },
];
