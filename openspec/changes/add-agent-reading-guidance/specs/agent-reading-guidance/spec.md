## ADDED Requirements

### Requirement: Follow explicit agent file activity

Repo City SHALL offer an opt-in Follow agent control. When enabled, activity with an explicit repository-relative file path SHALL navigate to that file and an available source line. The client SHALL NOT infer file activity from generated prose or invent a line number when an event has no line anchor; unanchored activity SHALL use the file start. Rapid events SHALL be coalesced. Disabling follow SHALL stop subsequent automatic navigation. Ordinary MCP proposals SHALL continue to require acceptance.

#### Scenario: Follow a file operation

- **GIVEN** Follow agent is enabled for the current checkout
- **WHEN** an agent event identifies a file in the current comparison
- **THEN** the city navigates to its source location and immediately shows the reading block
- **AND** a completed edit uses its changed-line anchor when available

#### Scenario: An event cannot resolve to a file

- **GIVEN** Follow agent is enabled
- **WHEN** activity has no explicit path or refers outside the comparison
- **THEN** the city leaves the current camera unchanged

### Requirement: Show agent-guided source without premature read credit

An accepted agent navigation SHALL show the source reading block immediately. The reading timer SHALL run only while the window is active, the block is visible and the camera has arrived. Looking elsewhere or opening an obstructing view SHALL pause the timer. Leaving the current source visit SHALL remove temporary visibility. Guided reading SHALL NOT require the heat tool. Completion SHALL use the existing configured reading estimate and persisted reveal mechanism.

#### Scenario: Inspect an agent-selected block

- **GIVEN** an unread block has a one-second reading estimate
- **WHEN** the reviewer accepts agent navigation and looks at it for 0.4 seconds
- **THEN** its code is visible and the block is still unread
- **WHEN** the reviewer looks away and returns for another 0.6 seconds
- **THEN** the same timer completes and the block is saved as read

### Requirement: Resume interrupted reading within the session

Source thawing and passive diff/blame reading SHALL retain unfinished reading time when focus moves elsewhere. Timers SHALL identify the file, content, comparison side and actual block fragments, independently of mutable file indices. Only unfinished timers SHALL be retained. Resetting progress SHALL clear them. Reopening the application MAY discard unfinished timers; completed reading SHALL remain persisted.

#### Scenario: Visit another file before returning

- **GIVEN** a source block has 0.4 seconds of a one-second delay
- **WHEN** the reviewer reads a different file and returns to the original unchanged block
- **THEN** another 0.6 seconds completes the original block
- **AND** time spent in the other file does not count toward it

#### Scenario: Content changes at the same location

- **GIVEN** a block has unfinished reading time
- **WHEN** its content changes before the reviewer returns
- **THEN** the new content starts with no reading time inherited from the old content

### Requirement: Show unread source through frosted glass

The city presentation SHALL show blurred source glyphs with their indentation, spacing and syntax colors instead of opaque unread bars. Both close source tiles and loaded distant previews SHALL use real source text. Line numbers and comparison markers SHALL remain clear. The same presentation SHALL run on native graphics and browser WebGPU. Opening or covering resident rows SHALL use a short blend; presentation SHALL NOT alter saved reading credit.

#### Scenario: Approach an unread file

- **GIVEN** a source wall with no completed reading
- **WHEN** its source preview and close tiles load
- **THEN** the reviewer sees blurred text shapes and syntax colors
- **AND** the completed reading count remains zero

### Requirement: Reveal accepted source ranges

MCP proposals SHALL accept an optional inclusive end line. Acceptance with Y or Fly SHALL immediately make that range legible on the source face, unfold its target, and show a warm highlight that fades within three seconds after arrival. Dismissal SHALL NOT navigate or reveal code. Proposals SHALL be bound to the checkout, displayed comparison, source blob, process and expiry. Changing the target's bytes before acceptance SHALL invalidate the proposal.

The browser and native comment Fly action SHALL use the same range presentation. An outdated comment without a current source anchor SHALL NOT offer a current-line flight. Guided visibility SHALL last for the current source visit; leaving the source visit clears it while retaining any completed reading.

#### Scenario: Inspect a comment before earning read credit

- **GIVEN** an unread six-line comment range and a sixty-second reading estimate
- **WHEN** the reviewer chooses Fly to line
- **THEN** the range is legible on arrival, with a warm cue
- **AND** the cue fades while the range stays legible
- **AND** navigation alone does not mark its lines read

### Requirement: Create private MCP range notes

The native MCP server SHALL expose create_note with a stable caller-provided id, displayed revision, repository-relative path, one-based start line, optional inclusive end line, side and text. It SHALL validate the range against actual source and bind the saved note to the current blob. An optional expected_blob SHALL reject stale observations. Repeating an identical request SHALL return the existing note and actual local status; reusing an id for different content SHALL fail. New local notes SHALL appear in the running city without restart. Creating a note SHALL NOT move the camera or publish to GitHub.

#### Scenario: Retry after a note is addressed

- **GIVEN** a local note created through MCP and later marked addressed
- **WHEN** its identical create request is retried
- **THEN** the note remains addressed and the response reports addressed
- **AND** no duplicate or remote comment is created

### Requirement: Keep read credit attached to observed content

Completed read ranges SHALL persist with their source blob and observed text fragments in the existing native SQLite or browser IndexedDB storage. Exact content SHALL restore its reading credit. Updated source MAY retain credit only for uniquely matched unchanged lines that were previously read; new or ambiguous lines SHALL remain unread. Legacy live marks without content identity SHALL NOT prove that current bytes were read. Resetting the current comparison SHALL remain effective after reopening it.

#### Scenario: Carry reading into edited source

- **GIVEN** one observed line and an unread line in a saved comparison
- **WHEN** a later revision inserts a line and rewrites another
- **THEN** only the uniquely matched observed line restores its reading credit
- **AND** new, rewritten and previously unread lines remain frosted

### Requirement: Keep the reading tool outside the text

The city presentation SHALL replace the large water stream with a compact heater near the lower-right edge and short warm pulses near its emitter and target. The existing countdown around the cursor SHALL remain the reading timer. Paint tools SHALL retain their annotation behavior. The heater SHALL NOT obscure the source reading area.

#### Scenario: Heat a block without obscuring its code

- **GIVEN** the heat tool is selected at an unread source wall
- **WHEN** the reviewer starts heating
- **THEN** a small tool stays near the lower-right edge and short warm pulses indicate activity
- **AND** the countdown remains around the cursor while the code stays unobstructed
