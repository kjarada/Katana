export const meta = {
  name: 'gdal-build-continue-2',
  description: 'Continue the GDAL build swarm (second restart): mid-merge wave 1, wave 2, integrate with main (smart leaders, IFC), prove every package, review, verify, fix',
  phases: [
    { title: 'Mid-merge', detail: 'confirm raster-toolbox green, merge the five wave-1 lanes onto F0, full build, full suite' },
    { title: 'Wave 2', detail: 'io-options (I3, I4) and analysis (T4, T5)' },
    { title: 'Integrate', detail: 'merge wave 2 and main, full build, full suite' },
    { title: 'Prove', detail: 'every package end to end in the window, katana_cli and katana_mcp, in three groups' },
    { title: 'Review', detail: 'three lenses: gdal, katana, surfaces-truth' },
    { title: 'Verify', detail: "adversarial check of each lens's findings" },
    { title: 'Fix', detail: 'fix what survived, rebuild, full suite' },
  ],
}

const PLAN = 'C:/GitHubProjects/Katana-wt/gdal-design.json'
const SCRATCH = 'C:/GitHubProjects/Katana-wt/gdal-scratch'
const REPORTS = 'C:/GitHubProjects/Katana-wt/gdal-wave1-reports.json'
const PROVE_DIR = 'C:/GitHubProjects/Katana-wt/gdal-prove'
const WT = 'C:/GitHubProjects/Katana-wt/gdal-mid'
const RULES = `
STANDING RULES - the repo's CLAUDE.md has them in full; the ones that bite here:
- Section 1: every feature on all three surfaces - the window (menu item, dialog with an objectName on every widget; the dialog builds the verb line and runs it through MainWindow::runVerbLine, the ONE executor), katana_cli and katana_mcp. One undo step per edit. key=value replies. No hidden modality.
- Section 1.1: every tool on drawing data takes the shared scope and filter (include/katana/cad/scope_verbs.hpp; in the window src/katana_qt/customisation/scope_filter_widget.*). Never a second scope/filter implementation.
- Section 11 layering: GDAL only in katana_io/gis/pointcloud and interop; cad may NOT see interop; no GDAL type in a public header.
- Work ONLY in worktrees under C:/GitHubProjects/Katana-wt/. Never touch C:/GitHubProjects/Katana's working tree, its build directory or its main branch: the owner is using them. main is only ever read here, as a merge source.
- Build: cmake --preset release in the worktree, then cmake --build build/release --parallel N (N=8 while two agents build at once, otherwise --parallel alone). Never two builds in one build dir; a build moved to the background keeps running - wait for its notification before building again. Show the build: cmake, ninja and ctest output unfiltered - no grep/tail/head over it (tee to a file if you need a copy, then search the file); judge by the EXIT CODE (with tee, bash's PIPESTATUS). Never pipe a build into Select-Object -First. "premature end of file; recovering" -> ninja -C build/release -t recompact, then build once. After editing CMake files run cmake --preset release on its own first. If configure tries to download GoogleTest/benchmark, copy C:/GitHubProjects/Katana/third_party/_cache into the worktree's third_party/ first.
- Tests: QT_QPA_PLATFORM=offscreen ctest --test-dir build/release --parallel N --output-on-failure. Section 4: never change an expected value to match output - justify any moved expectation from a source outside the program; never disable a test; prove a regression test catches its bug (remove the fix, watch it fail, restore). -Werror -Wshadow. Match the surrounding style; test names are sentences.
- Naming (section 9): no "12d", "Transport for NSW"/"TfNSW" in new identifiers/UI/docs/commits (the public TfNSW Utility Schema in the utility tools excepted).
- Docs in the same commit; the docs test green. Commits "<area>: <what>" + why + Evidence + Outstanding, ending with the line: Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>. git diff --cached before each commit; never git add -A blindly. Never push; never rewrite history.
- THE DESIGN: ${PLAN} (plan.architecture, plan.contract, plan.packages[], plan.notDoing, plan.risks, and the api/current/raster/vector reports). The API recipe and proven scratch programs are in ${SCRATCH}. The wave-1 lanes' reports are in ${REPORTS}. Heed plan.risks (cancel reported as success, errors on GDAL worker threads, never share a dataset between concurrent runs, thread-local config, big outputs spill to files, destructive algorithms gated from MCP).
- CONTEXT: this swarm started from main 2dd37cf. F0 is on branch gdal-f0; wave 1 finished on branches gdal-gis-verbs (I0 D1 D2), gdal-io-adapter (I1 I2), gdal-terrain (T0-T3), gdal-raster-toolbox (T6 T7 X1 X2), gdal-vector (V1 V4 V2 V5 V3), each in its worktree C:/GitHubProjects/Katana-wt/<branch>. Two sessions that ran it have ended; this workflow continues it. If a branch or worktree this step would create already exists, inspect it and continue from its state rather than recreating it.
`
const RASTER = `raster-toolbox lane (branch gdal-raster-toolbox, HEAD 3b155a2, worktree clean, built). It has no structured report: its finishing agent stopped during a final confirming run. What its transcript shows:
- Packages, all committed: T6 RASTER GRID (4f38d48), T7 DEM tools (3b1ae14), X1 GDAL Toolbox (337abad), X2 the Toolbox's Pipeline tab (097a608).
- Fixes its verification found: 62d7c50 the GDAL Toolbox and the DEM submenu take menu letters no other item has (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless and qt_the_help_menu_finds_a_verb_and_lists_every_key_headless had failed); 30ca2ea the scratch folder is one per process run, not per process id (derived names collided between parallel test processes, so cli.raster_clip_of_the_plane_to_an_area_is_twenty_by_fifteen and GeoSession.AGdalLineRunsThroughTheSessionAndItsRasterIsAReference flaked; a regression test was shown to fail without the fix); 3b155a2 geo_dialog_support.cpp includes <algorithm> for std::any_of.
- Its checks: the grid verb reads scope with cad::parseScopeWords and binds through F0's bindDrawing; the DEM dialogs use ScopeFilterWidget; the Toolbox frame runs only through the window's executor; objectNames match the design; MCP reach is tested for both verb families; file actions have headless paths; list arguments are validated per item; terrain.md records the GDAL 3.13.2 CRS count (7216 EPSG entries).
- Whole suite 4905/4905 at 11:57 local with every change in the working tree; the confirming run on the committed HEAD was cut off.`

const FINDING = { type: 'object', properties: { id: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, file: { type: 'string' }, line: { type: 'integer' }, claim: { type: 'string' }, evidence: { type: 'string' }, suggestedFix: { type: 'string' } }, required: ['id', 'severity', 'file', 'claim', 'evidence', 'suggestedFix'] }
const FINDINGS = { type: 'object', properties: { findings: { type: 'array', items: FINDING } }, required: ['findings'] }
const PKG = {
  type: 'object',
  properties: {
    branch: { type: 'string' }, worktree: { type: 'string' }, headCommit: { type: 'string' },
    packages: { type: 'array', items: { type: 'object', properties: { name: { type: 'string' }, status: { type: 'string', enum: ['done', 'partial', 'not_started'] }, commits: { type: 'array', items: { type: 'string' } }, surfaces: { type: 'string' }, tests: { type: 'string' }, deviations: { type: 'string' }, outstanding: { type: 'string' } }, required: ['name', 'status', 'commits', 'surfaces', 'tests', 'deviations', 'outstanding'] } },
    buildOk: { type: 'boolean' },
    summary: { type: 'string' },
  },
  required: ['branch', 'worktree', 'headCommit', 'packages', 'buildOk', 'summary'],
}
const MERGE = {
  type: 'object',
  properties: {
    branch: { type: 'string' }, worktree: { type: 'string' }, headCommit: { type: 'string' },
    merged: { type: 'array', items: { type: 'string' } },
    testsPassed: { type: 'integer' }, testsFailed: { type: 'integer' }, failures: { type: 'array', items: { type: 'string' } },
    fixesMade: { type: 'array', items: { type: 'string' } },
    packageStatus: { type: 'array', items: { type: 'object', properties: { name: { type: 'string' }, status: { type: 'string' }, windowPath: { type: 'string' }, cli: { type: 'string' }, mcp: { type: 'string' } }, required: ['name', 'status', 'windowPath', 'cli', 'mcp'] } },
    summary: { type: 'string' },
  },
  required: ['branch', 'worktree', 'headCommit', 'merged', 'testsPassed', 'testsFailed', 'failures', 'fixesMade', 'packageStatus', 'summary'],
}
const PROVE = {
  type: 'object',
  properties: {
    packages: { type: 'array', items: { type: 'object', properties: { name: { type: 'string' }, status: { type: 'string', enum: ['proven', 'partial', 'failed'] }, window: { type: 'string' }, cli: { type: 'string' }, mcp: { type: 'string' } }, required: ['name', 'status', 'window', 'cli', 'mcp'] } },
    findings: { type: 'array', items: FINDING },
  },
  required: ['packages', 'findings'],
}
const VERDICTS = {
  type: 'object',
  properties: { verdicts: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, verdict: { type: 'string', enum: ['confirmed', 'plausible', 'refuted'] }, reason: { type: 'string' }, evidence: { type: 'string' } }, required: ['id', 'verdict', 'reason', 'evidence'] } } },
  required: ['verdicts'],
}
const FIX = {
  type: 'object',
  properties: { headCommit: { type: 'string' }, fixed: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, what: { type: 'string' } }, required: ['id', 'what'] } }, refuted: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, why: { type: 'string' } }, required: ['id', 'why'] } }, deferred: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, why: { type: 'string' } }, required: ['id', 'why'] } }, testsPassed: { type: 'integer' }, testsFailed: { type: 'integer' }, failures: { type: 'array', items: { type: 'string' } }, katanaExeNewerThanSources: { type: 'boolean' }, summary: { type: 'string' } },
  required: ['headCommit', 'fixed', 'refuted', 'deferred', 'testsPassed', 'testsFailed', 'failures', 'katanaExeNewerThanSources', 'summary'],
}

phase('Mid-merge')
const mid = await agent(`You merge wave 1 of the GDAL build swarm for Katana.
${RULES}
Wave-1 lane reports: read ${REPORTS} (F0, gis-verbs, io-adapter, terrain, vector). The raster-toolbox lane has none; here is what is known of it:
${RASTER}
0. Confirm the raster-toolbox lane first: in C:/GitHubProjects/Katana-wt/gdal-raster-toolbox run cmake --build build/release --parallel (expect a no-op), then the WHOLE suite. If anything is red, fix it on that branch and commit before merging.
1. git -C C:/GitHubProjects/Katana worktree add -b gdal-mid ${WT} gdal-f0 ; then in ${WT} merge --no-ff (messages ending with the attribution line), in order: gdal-gis-verbs, gdal-io-adapter, gdal-terrain, gdal-raster-toolbox, gdal-vector. Resolve every conflict keeping every lane's intent (menus and their letters, the GDAL/GIS/RASTER verb dispatch, CMake lists, the explicit source lists of tests/qt_widgets and benchmarks/qt, MCP tools, docs).
2. cmake --preset release; FULL build (--parallel - nothing else is building); FULL suite. Fix every failure properly, commit each fix, rerun until green.
Return the structured result (packageStatus: one line per wave-1 package, F0 included).`, { label: 'mid-merge', phase: 'Mid-merge', schema: MERGE })
log(`mid: ${mid ? `${mid.testsPassed} passed / ${mid.testsFailed} failed at ${mid.headCommit}` : 'FAILED'}`)
if (!mid) {
  return { stopped: 'mid-merge failed' }
}

phase('Wave 2')
const WAVE2 = [
  { key: 'io-options', pkgs: ['I3', 'I4'] },
  { key: 'analysis', pkgs: ['T4', 'T5'] },
]
const wave2 = await parallel(WAVE2.map(l => () => agent(`You are lane "${l.key}" of the GDAL build swarm for Katana.
${RULES}
Setup: git -C C:/GitHubProjects/Katana worktree add -b gdal-${l.key} C:/GitHubProjects/Katana-wt/gdal-${l.key} gdal-mid ; configure and build it once (--parallel 8). gdal-mid is F0 plus all of wave 1, merged and green. The mid-merge reported: ${JSON.stringify(mid).slice(0, 8000)}
Your packages, IN THIS ORDER (ids as in ${PLAN}; a package's id is the first word of its name): ${l.pkgs.join(', ')}. Read each one's design, ownedFiles, sharedHotspots and tests.
The other wave-2 lane builds at the same time; stay inside your packages' owned files plus the small exact hotspot edits their designs list; do not reformat code you do not need to change.
For EACH package: verify the need still exists in the current code; implement it on all three surfaces with the shared scope where it acts on drawing data; tests (unit, verb, MCP, widget by objectName, a headless end-to-end check where the window path is the point); docs; incremental build (--parallel 8); the relevant tests plus layering and docs; commit. Before returning, run ctest over every test area your packages touched.
Return the structured result, one entry per package.`, { label: `lane ${l.key}: ${l.pkgs.join(' ')}`, phase: 'Wave 2', schema: PKG })))
const w2 = wave2.map((r, i) => ({ lane: WAVE2[i].key, branch: `gdal-${WAVE2[i].key}`, result: r }))
for (const x of w2) log(`lane ${x.lane}: ${x.result ? x.result.packages.map(p => `${p.name.split(' ')[0]}=${p.status}`).join(', ') : 'FAILED'}`)

phase('Integrate')
const integrated = await agent(`You integrate the GDAL build swarm for Katana, and bring it level with main.
${RULES}
Wave-2 lane reports: ${JSON.stringify(w2).slice(0, 30000)}
1. In ${WT} (branch gdal-mid, built): git branch gdal gdal-mid ; git switch gdal ; merge --no-ff gdal-io-options then gdal-analysis (skip a lane whose report above is null, and say so). Resolve conflicts keeping both lanes' intent.
2. Then merge local main into it as it is now: git merge --no-ff main (record main's hash in the summary). main has moved since the swarm started (2dd37cf), at least by: PR #7 smart leaders (leader tip put ON an entity, field templates, the Leaders manager, include/katana/cad/annotation/leader_edit.hpp) and PR #8 IFC 4.3 (a new katana_ifc module; IMPORT/EXPORT/INFO <file.ifc> and IFC RULES in katana_app/session.cpp - its IFC hook runs before the other IMPORT handling - and in the window, where MainWindow's dispatch sends a .ifc line to runIfcLine ABOVE the IMPORT placement block so its options are not read as a path; File > Import IFC / Export IFC; WrittenCells in utility_network.hpp). If main has commits beyond b749688, read them too. Expect conflicts between GDAL's I0 (one GIS executor for IMPORT/EXPORT/INFO/REFS/COPC) and IFC's grammar: the result must keep .ifc lines going to the IFC grammar first on every front end (window, katana_cli, katana_mcp), and keep every GDAL change. Keep the File menu's letters unique (qt_every_shortcut_and_menu_letter_reaches_one_thing_headless).
3. cmake --preset release; FULL build (--parallel); FULL suite. Fix every failure properly (CLAUDE.md section 4), commit, rerun until green.
4. A smoke check that both grammars survived: from katana_cli, one GDAL IMPORT or INFO of a GeoPackage or GeoTIFF, and one IFC EXPORT then IMPORT of a .ifc file. (Separate provers will prove every package in depth after you.)
Return the structured result (packageStatus: one line per package you can vouch for from the suite; the provers do the rest).`, { label: 'integrate', phase: 'Integrate', schema: MERGE })
log(`integrated: ${integrated ? `${integrated.testsPassed} passed / ${integrated.testsFailed} failed at ${integrated.headCommit}` : 'FAILED'}`)
if (!integrated) {
  return { mid, w2, stopped: 'integration failed' }
}

const SHARED = `Nothing builds while you work, but two or three other agents run the same binaries in ${WT}/build/release at the same time: do NOT edit source, build or commit, never run the whole ctest suite (a narrow ctest -R is fine), and work only in your own folder under ${PROVE_DIR} on COPIES of data.`
const GROUPS = [
  { key: 'io', pkgs: 'F0 I0 I1 I2 I3 I4 D1 D2', extra: 'Also prove IFC still works after the merge: EXPORT and IMPORT of a .ifc file from katana_cli, katana_mcp and the window (File > Export IFC / Import IFC), and that an INFO/IMPORT/EXPORT line on a .ifc file reaches the IFC grammar on every front end while the same verbs on a GeoPackage, shapefile or GeoTIFF reach the GDAL executor.' },
  { key: 'terrain', pkgs: 'T0 T1 T2 T3 T4 T5 T6 T7', extra: '' },
  { key: 'vector-toolbox', pkgs: 'V1 V2 V3 V4 V5 X1 X2', extra: '' },
]
const provePrompt = g => `You prove, end to end, packages ${g.pkgs} of the GDAL integration of Katana, in ${WT} (branch gdal, built at ${integrated.headCommit}, full suite ${integrated.testsPassed} passed / ${integrated.testsFailed} failed). Each package's design is in ${PLAN} (a package's id is the first word of its name); the lanes' reports: ${REPORTS} and ${JSON.stringify(w2.map(x => ({ lane: x.lane, packages: x.result ? x.result.packages : null }))).slice(0, 12000)}
${SHARED} Your folder: ${PROVE_DIR}/${g.key}. Data: copies of samples/site_plan, samples/gis, or data you generate.
For EACH package prove each surface separately, keeping the output as evidence:
- the window, headlessly (QT_QPA_PLATFORM=offscreen, under timeout; build/release/bin/katana.exe with --action/--dialog/--fill/--press/--report/--command/--run-line/--screenshot, see docs/headless.md): reach the menu item and dialog by objectName, run it, and read the PNG where the result is visual (contours drawn, hillshade shown, buffer made); check its scope controls where it reads drawing data;
- katana_cli -c lines, including the scope forms (AREA, LAYERS, DRAWING, WHERE; VIEW and SELECTION refused headless) and at least one documented failure;
- katana_mcp: its tools and resources and verb lines through it, with honest JSON (a JSON-RPC session over stdin, as tests/app/test_mcp_server.cpp does).
${g.extra}
Check numbers against an independent calculation, not against the program. A surface that does not work, a wrong result, a missing objectName, a failure that blocks or is silent: each is a finding with its exact reproduction, id prefixed "prove-${g.key}-". Return the structured result: one packages entry per package, and the findings.`

const base = `Review the GDAL integration of Katana in ${WT} (branch gdal, built at ${integrated.headCommit}): git -C ${WT} diff main...HEAD, reading touched files whole where needed. The design is ${PLAN}. ${SHARED} Your folder: ${PROVE_DIR}/review-<your lens>. Report only real defects with concrete evidence (file:line, code, the input that breaks it); prefix ids with your lens.`
const LENSES = [
  { key: 'gdal', lens: `LENS "gdal": correctness against GDAL's behaviour and plan.risks - cancel reported as success with partial output, errors from GDAL worker threads lost, datasets shared between concurrent runs, thread-local config assumptions, memory on big outputs, CRS mismatches and LOCAL-shifted data, Z lost, Float32 precision, destructive algorithms reachable from MCP, agent-safety options (SPATIALITE_SECURITY, GDAL_ENABLE_EXTERNAL, --config), the data-loss fixes (CRS written, KML/CSV geometry, MapInfo precision, holes, typed attributes) fixed with tests; and that IFC's .ifc lines still reach the IFC grammar first everywhere.` },
  { key: 'katana', lens: `LENS "katana": the executor and undo (one step per apply, atomic, refused when the drawing changed underneath), the shared scope used everywhere drawing data is read (no second scope/filter implementation), the verb grammars and replies, the one window executor (no second path), threading and lifetimes of the new dialogs and background jobs, layering, tests that would pass if the code were wrong, silent failures, numbers with no source.` },
  { key: 'surfaces', lens: `LENS "surfaces-truth": every package of ${PLAN} reachable on all three surfaces (menu paths and objectNames, katana_cli, katana_mcp tools with honest JSON schemas); HELP, the Command Reference, katana_cli --help and the MCP descriptions complete and true; docs and comments true of the code; the naming rule; the docs test green; plan.notDoing items not half-done. Do not report the attribution trailer of already-made commits.` },
]
const verifyPrompt = (key, fs) => `You adversarially check the findings of the "${key}" review of the GDAL integration of Katana in ${WT} (branch gdal, built at ${integrated.headCommit}).
${SHARED} Your folder: ${PROVE_DIR}/verify-${key}.
For EACH finding, try to REFUTE it: read the code at the cited place and around it, and reproduce the claimed failure with the input given (a katana_cli line, a headless katana.exe run under timeout with QT_QPA_PLATFORM=offscreen, a narrow ctest -R). Verdict: confirmed (you reproduced it, or the code plainly does it), plausible (the code reads that way but you could not reproduce it), refuted (the evidence does not hold - say why). When torn between plausible and refuted, choose refuted and say why. One verdict per finding id.
Findings: ${JSON.stringify(fs, null, 1).slice(0, 60000)}`

const CHECKERS = [
  ...GROUPS.map(g => ({ kind: 'prove', key: g.key, phase: 'Prove', label: `prove:${g.key}`, prompt: provePrompt(g), schema: PROVE })),
  ...LENSES.map(l => ({ kind: 'review', key: l.key, phase: 'Review', label: `review:${l.key}`, prompt: `${base}\n${l.lens}`, schema: FINDINGS })),
]
const checked = await pipeline(CHECKERS,
  c => agent(c.prompt, { label: c.label, phase: c.phase, schema: c.schema }),
  async (r, c) => {
    if (!r) {
      log(`${c.label}: FAILED (no result)`)
      return { c, r: null, kept: [], refuted: [] }
    }
    const fs = r.findings || []
    if (c.kind === 'prove') {
      log(`${c.label}: ${r.packages.map(p => `${p.name.split(' ')[0]}=${p.status}`).join(', ')}; ${fs.length} findings`)
      return { c, r, kept: fs.map(f => ({ ...f, source: c.label, verdict: 'reproduced by the prover' })), refuted: [] }
    }
    if (fs.length === 0) return { c, r, kept: [], refuted: [] }
    const v = await agent(verifyPrompt(c.key, fs), { label: `verify:${c.key}`, phase: 'Verify', schema: VERDICTS })
    const byId = new Map((v ? v.verdicts : []).map(x => [x.id, x]))
    const kept = [], refuted = []
    for (const f of fs) {
      const x = byId.get(f.id)
      if (x && x.verdict === 'refuted') refuted.push({ ...f, source: c.label, refutation: x.reason, refutationEvidence: x.evidence })
      else kept.push({ ...f, source: c.label, verdict: x ? `${x.verdict}: ${x.reason} | ${x.evidence}` : 'unverified (no verdict returned)' })
    }
    log(`${c.label}: ${fs.length} findings, ${kept.length} survived verification, ${refuted.length} refuted`)
    return { c, r, kept, refuted }
  })
const done = checked.filter(Boolean)
const proofs = done.filter(x => x.c.kind === 'prove').map(x => ({ group: x.c.key, packages: x.r ? x.r.packages : null }))
const kept = done.flatMap(x => x.kept)
const refuted = done.flatMap(x => x.refuted)
const failedCheckers = CHECKERS.filter((c, i) => !checked[i] || !checked[i].r).map(c => c.label)
if (failedCheckers.length) log(`checkers with no result: ${failedCheckers.join(', ')}`)
log(`${kept.length} findings to fix (${kept.filter(f => f.severity === 'high').length} high); ${refuted.length} refuted`)

phase('Fix')
const fixed = kept.length === 0 ? { headCommit: integrated.headCommit, fixed: [], refuted: [], deferred: [], testsPassed: integrated.testsPassed, testsFailed: integrated.testsFailed, failures: [], katanaExeNewerThanSources: true, summary: 'no findings survived' } :
  await agent(`You finish the GDAL integration of Katana in ${WT} (branch gdal, built at ${integrated.headCommit}).
${RULES}
(Builds here may use --parallel without a number - nothing else is building now.)
Each finding below was either reproduced by a prover or survived an adversarial check (its verdict says which). Still read before changing anything; if one proves wrong on closer reading, list it as refuted with the evidence.
Findings: ${JSON.stringify(kept, null, 1).slice(0, 90000)}
Fix every real one properly, with a test that fails before and passes after where the defect is behavioural; commit in small groups, docs in the same commit. For a prover's finding, rerun its reproduction after the fix. Defer only what is genuinely out of scope, saying why, and record it under "Not done" in the area's doc. Then cmake --preset release, full build, FULL suite green, and confirm build/release/bin/katana.exe is newer than every source you changed.
Return the structured result.`, { label: 'fix', phase: 'Fix', schema: FIX })
log(`fix: ${fixed ? `${fixed.fixed.length} fixed, ${fixed.refuted.length} refuted, ${fixed.deferred.length} deferred; ${fixed.testsPassed} passed / ${fixed.testsFailed} failed at ${fixed.headCommit}` : 'FAILED'}`)

return { mid, w2, integrated, proofs, kept, refuted, failedCheckers, fixed }
