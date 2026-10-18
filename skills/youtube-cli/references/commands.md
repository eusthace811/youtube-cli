# youtube command reference

Every command and flag of the `youtube` command (youtube-cli), generated from the same registry the program is built from. `youtube <command> --help` prints the same flags.

Commands that change the channel ask for confirmation, and in a non-terminal shell they need `--yes`; add it only after the user agrees to that exact command.

## `youtube abuseReport`

Manage YouTube abuse reports.

### `youtube abuseReport insert`

Create an abuse report. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-a, --abuseTypes` | strings |  | Abuse type IDs (e.g., spam, harassment) **Required.** |
| `-d, --description` | string |  | Description of the abuse |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet]` | Parts to include |
| `-r, --relatedEntityId` | string |  | ID of a related entity |
| `-s, --subjectId` | string |  | ID of the subject being reported **Required.** |
| `-t, --subjectTypeId` | string |  | Type ID of the subject (e.g., video, comment, channel) **Required.** |
| `-u, --subjectUrl` | string |  | URL of the subject being reported |
| `--yes` | bool |  | Confirm the operation |

```bash
# Report a video as spam
youtube abuseReport insert --abuseTypes spam --subjectId VIDEO_ID --subjectTypeId video --description "This video is spam"
# Report a comment as harassment
youtube abuseReport insert --abuseTypes harassment --subjectId COMMENT_ID --subjectTypeId comment --description "Harassment in comment"
```

## `youtube activity`

Manage YouTube activities.

### `youtube activity list`

List activities. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | ID of the channel |
| `-F, --for` | string |  | home\|mine |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet,contentDetails]` | Comma separated parts |
| `-a, --publishedAfter` | string |  | Filter on activities published after this date |
| `-b, --publishedBefore` | string |  | Filter on activities published before this date |
| `-r, --regionCode` | string |  | Display the content as seen by viewers in this country |

```bash
# List my activities
youtube activity list --for mine
# List activities by channel ID with limit
youtube activity list --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --maxResults 10
# List activities after a date in JSON format
youtube activity list --publishedAfter 2024-01-01T00:00:00Z --output json
```

## `youtube caption`

Manage YouTube video captions.

### `youtube caption delete`

Delete captions. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the captions to delete **Required.** |
| `-b, --onBehalfOf` | string |  | ID of the YouTube account that the content owner is acting on behalf of |
| `-B, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a caption by ID
youtube caption delete --ids abc123
# Delete multiple captions
youtube caption delete --ids abc123,def456
```

### `youtube caption download`

Download a caption. Writes a local file: confirm the path, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-f, --file` | string |  | Path to save the caption file **Required.** |
| `-i, --id` | strings |  | ID of the caption to download **Required.** |
| `-b, --onBehalfOf` | string |  | ID of the YouTube account that the content owner is acting on behalf of |
| `-B, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-t, --tfmt` | string |  | sbv\|srt\|vtt |
| `-l, --tlang` | string |  | Translate the captions into this language |
| `--yes` | bool |  | Confirm the operation |

```bash
# Download a caption as SRT
youtube caption download --id abc123 --file subtitle.srt
# Download a caption as VTT
youtube caption download --id abc123 --file subtitle.vtt --tfmt vtt
# Download a caption translated to French
youtube caption download --id abc123 --file subtitle.srt --tlang fr
```

### `youtube caption insert`

Upload a caption. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-a, --audioTrackType` | string | `unknown` | unknown\|primary\|commentary\|descriptive |
| `-f, --file` | string |  | Path to save the caption file **Required.** |
| `-A, --isAutoSynced` | bool | `true` | Whether YouTube synchronized the caption track to the audio track in the video |
| `-C, --isCC` | bool |  | Whether the track contains closed captions for the deaf and hard of hearing |
| `-D, --isDraft` | bool |  | whether the caption track is a draft |
| `-E, --isEasyReader` | bool |  | Whether caption track is formatted for 'easy reader' |
| `-L, --isLarge` | bool |  | Whether the caption track uses large text for the vision-impaired |
| `-l, --language` | string |  | Language of the caption track |
| `-n, --name` | string |  | Name of the caption track |
| `-b, --onBehalfOf` | string |  | ID of the YouTube account that the content owner is acting on behalf of |
| `-B, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-t, --trackKind` | string | `standard` | standard\|ASR\|forced |
| `-v, --videoId` | string |  | ID of the video **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Insert a caption to a video
youtube caption insert --file subtitle.srt --videoId dQw4w9WgXcQ
# Insert a caption with language and name
youtube caption insert --file subtitle.srt --videoId dQw4w9WgXcQ --language en --name English
# Insert a standard caption and publish immediately
youtube caption insert --file subtitle.srt --videoId dQw4w9WgXcQ --trackKind standard --isDraft=false
```

### `youtube caption list`

List captions. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the captions to list |
| `-b, --onBehalfOf` | string |  | ID of the YouTube account that the content owner is acting on behalf of |
| `-B, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |
| `-v, --videoId` | string |  | ID of the video |

```bash
# List captions of a video
youtube caption list --videoId dQw4w9WgXcQ
# List captions in JSON format
youtube caption list --videoId dQw4w9WgXcQ --output json
# List specific captions by IDs
youtube caption list --ids abc123,def456 --videoId dQw4w9WgXcQ
```

### `youtube caption update`

Update a caption. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-a, --audioTrackType` | string | `unknown` | unknown\|primary\|commentary\|descriptive |
| `-f, --file` | string |  | Path to save the caption file |
| `-A, --isAutoSynced` | bool | `true` | Whether YouTube synchronized the caption track to the audio track in the video |
| `-C, --isCC` | bool |  | Whether the track contains closed captions for the deaf and hard of hearing |
| `-D, --isDraft` | bool |  | whether the caption track is a draft |
| `-E, --isEasyReader` | bool |  | Whether caption track is formatted for 'easy reader' |
| `-L, --isLarge` | bool |  | Whether the caption track uses large text for the vision-impaired |
| `-l, --language` | string |  | Language of the caption track |
| `-n, --name` | string |  | Name of the caption track |
| `-b, --onBehalfOf` | string |  | ID of the YouTube account that the content owner is acting on behalf of |
| `-B, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-t, --trackKind` | string | `standard` | standard\|ASR\|forced |
| `-v, --videoId` | string |  | ID of the video **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Publish a draft caption
youtube caption update --videoId dQw4w9WgXcQ --isDraft=false
# Update caption language and name
youtube caption update --videoId dQw4w9WgXcQ --language en --name English
# Update caption file
youtube caption update --videoId dQw4w9WgXcQ --file updated.srt
```

## `youtube channel`

Manage YouTube channels.

### `youtube channel list`

List channels. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-g, --categoryId` | string |  | Return the channels within the specified guide category id |
| `-F, --for` | string |  | managedByMe\|mine\|mySubscribers |
| `-d, --forHandle` | string |  | Return the channel associated with a YouTube handle |
| `-u, --forUsername` | string |  | Return the channel associated with a YouTube username |
| `-l, --hl` | string |  | Specifies the localization language of the metadata |
| `-i, --ids` | strings |  | Return the channels with the specified Ids |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet,status]` | Comma separated parts |

```bash
# List my channel info
youtube channel list --for mine
# List channel by handle
youtube channel list --forHandle @GoogleDevelopers
# List channel by ID in JSON format
youtube channel list --ids UC_x5XG1OV2P6uZZ5FSM9Ttw --output json
# List channels in table format
youtube channel list --maxResults 10 --output table
```

### `youtube channel update`

Update a channel. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --country` | string |  | Country of the channel |
| `-u, --customUrl` | string |  | Custom URL of the channel |
| `-l, --defaultLanguage` | string |  | The language of the channel's default title and description |
| `-d, --description` | string |  | Description of the channel |
| `-i, --id` | strings |  | ID of the channel to update **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-t, --title` | string |  | Title of the channel |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update channel description
youtube channel update --id UC_x5XG1OV2P6uZZ5FSM9Ttw --description 'New description'
# Update channel title and country
youtube channel update --id UC_x5XG1OV2P6uZZ5FSM9Ttw --title 'New Title' --country US
# Update channel default language
youtube channel update --id UC_x5XG1OV2P6uZZ5FSM9Ttw --defaultLanguage en
```

## `youtube channelBanner`

Manage YouTube channel banners.

### `youtube channelBanner insert`

Upload a channel banner. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | ID of the channel to insert the banner for **Required.** |
| `-f, --file` | string |  | Path to the banner image **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | YouTube channel ID linked to the content owner |
| `-o, --output` | string |  | json\|yaml\|silent |
| `--yes` | bool |  | Confirm the operation |

```bash
# Upload a channel banner
youtube channelBanner insert --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --file banner.jpg
# Upload a channel banner with JSON output
youtube channelBanner insert --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --file banner.png --output json
```

## `youtube channelSection`

Manage YouTube channel sections.

### `youtube channelSection delete`

Delete channel sections. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | Delete the channel sections with the given ids **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a channel section by ID
youtube channelSection delete --ids abc123
# Delete multiple channel sections
youtube channelSection delete --ids abc123,def456
```

### `youtube channelSection list`

List channel sections. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | Return the ChannelSections owned by the specified channel id |
| `-l, --hl` | string |  | Return content in specified language |
| `-i, --ids` | strings |  | Return the channel sections with the given ids |
| `-M, --mine` | bool |  | Return the ChannelSections owned by the authenticated user |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |

```bash
# List my channel sections
youtube channelSection list --mine
# List channel sections by channel ID
youtube channelSection list --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw
# List specific channel sections in JSON format
youtube channelSection list --ids abc123,def456 --output json
```

## `youtube comment`

Manage YouTube comments.

### `youtube comment delete`

Delete comments. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of comments **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a comment by ID
youtube comment delete --ids abc123
# Delete multiple comments
youtube comment delete --ids abc123,def456
```

### `youtube comment insert`

Create a comment. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-a, --authorChannelId` | string |  | Channel id of the comment author |
| `-R, --canRate` | bool |  | Whether the viewer can rate the comment |
| `-c, --channelId` | string |  | Channel id of the video owner |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-P, --parentId` | string |  | ID of the parent comment **Required.** |
| `-t, --textOriginal` | string |  | Text of the comment **Required.** |
| `-v, --videoId` | string |  | ID of the video |
| `--yes` | bool |  | Confirm the operation |

```bash
# Reply to a comment
youtube comment insert --channelId UC_x5X --videoId dQw4w9 --authorChannelId UA_x5X --parentId UgyXXX --textOriginal 'Hello'
# Reply with rating enabled
youtube comment insert --channelId UC_x5X --videoId dQw4w9 --authorChannelId UA_x5X --parentId UgyXXX --textOriginal 'Reply' --canRate
```

### `youtube comment list`

List comments. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of comments |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-P, --parentId` | string |  | Returns replies to the specified comment |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |
| `-t, --textFormat` | string | `html` | textFormatUnspecified\|html\|plainText |

```bash
# List replies to a comment
youtube comment list --parentId UgyXXXXXXXX --maxResults 10
# List specific comments in JSON format
youtube comment list --ids abc123,def456 --output json
# List a comment in plain text
youtube comment list --ids abc123 --textFormat plainText
```

### `youtube comment markAsSpam`

Mark comments as spam. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of comments **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `--yes` | bool |  | Confirm the operation |

```bash
# Mark a comment as spam
youtube comment markAsSpam --ids abc123
# Mark multiple comments as spam
youtube comment markAsSpam --ids abc123,def456
```

### `youtube comment setModerationStatus`

Set comment moderation status. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-A, --banAuthor` | bool |  | If set to true the author of the comment gets added to the ban list |
| `-i, --ids` | strings |  | IDs of comments **Required.** |
| `-s, --moderationStatus` | string |  | heldForReview\|published\|rejected **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `--yes` | bool |  | Confirm the operation |

```bash
# Publish a held comment
youtube comment setModerationStatus --ids abc123 --moderationStatus published
# Hold multiple comments for review
youtube comment setModerationStatus --ids abc123,def456 --moderationStatus heldForReview
# Reject a comment and ban author
youtube comment setModerationStatus --ids abc123 --moderationStatus rejected --banAuthor
```

### `youtube comment update`

Update a comment. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-R, --canRate` | bool |  | Whether the viewer can rate the comment |
| `-i, --id` | strings |  | ID of the comment **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-t, --textOriginal` | string |  | Text of the comment |
| `-r, --viewerRating` | string |  | none\|like\|dislike |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update comment text
youtube comment update --id abc123 --textOriginal 'Updated comment'
# Like a comment
youtube comment update --id abc123 --viewerRating like
# Update comment text with rating enabled
youtube comment update --id abc123 --textOriginal 'New text' --canRate
```

## `youtube commentThread`

Manage YouTube comment threads.

### `youtube commentThread insert`

Create a comment thread. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-a, --authorChannelId` | string |  | Channel id of the comment author |
| `-c, --channelId` | string |  | Channel id of the video owner **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-t, --textOriginal` | string |  | Text of the comment **Required.** |
| `-v, --videoId` | string |  | ID of the video **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Post a comment on a video
youtube commentThread insert --channelId UC_x5X --videoId dQw4w9WgXcQ --authorChannelId UA_x5X --textOriginal 'Great video!'
# Post a comment with JSON output
youtube commentThread insert --channelId UC_x5X --videoId dQw4w9WgXcQ --authorChannelId UA_x5X --textOriginal 'Nice work!' --output json
```

### `youtube commentThread list`

List comment threads. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-a, --allThreadsRelatedToChannelId` | string |  | Returns the comment threads of all videos of the channel and the channel comments as well |
| `-c, --channelId` | string |  | Channel id of the video owner |
| `-i, --ids` | strings |  | IDs of the comment threads |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-m, --moderationStatus` | string | `published` | published\|heldForReview\|likelySpam\|rejected |
| `-O, --order` | string | `time` | orderUnspecified\|time\|relevance |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |
| `-s, --searchTerms` | string |  | Search terms |
| `-t, --textFormat` | string | `html` | textFormatUnspecified\|html |
| `-v, --videoId` | string |  | Returns the comment threads of the specified video |

```bash
# List comment threads on a video
youtube commentThread list --videoId dQw4w9WgXcQ --maxResults 10
# Search comment threads by terms
youtube commentThread list --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --searchTerms 'hello'
# List specific comment threads in JSON format
youtube commentThread list --ids abc123,def456 --output json
```

## `youtube i18nLanguage`

Manage YouTube i18n languages.

### `youtube i18nLanguage list`

List i18n languages. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --hl` | string |  | Host language |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |

## `youtube i18nRegion`

Manage YouTube i18n regions.

### `youtube i18nRegion list`

List i18n regions. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --hl` | string |  | Host language |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |

## `youtube liveBroadcast`

Manage YouTube live broadcasts.

### `youtube liveBroadcast bind`

Bind a live broadcast to a stream. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the broadcasts to bind **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[id,snippet,contentDetails,status]` | Comma separated parts |
| `-s, --streamId` | string |  | ID of the live stream to bind |
| `--yes` | bool |  | Confirm the operation |

```bash
# Bind a broadcast to a stream
youtube liveBroadcast bind --ids broadcast123 --streamId stream456
# Unbind a broadcast (omit streamId)
youtube liveBroadcast bind --ids broadcast123
```

### `youtube liveBroadcast delete`

Delete live broadcasts. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the live broadcasts to delete **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a live broadcast by ID
youtube liveBroadcast delete --ids broadcast123
# Delete multiple live broadcasts
youtube liveBroadcast delete --ids broadcast123,broadcast456
```

### `youtube liveBroadcast insert`

Create a live broadcast. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d, --description` | string |  | Description of the live broadcast |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet,status,contentDetails]` | Comma separated parts |
| `-P, --privacyStatus` | string |  | Privacy status (public, unlisted, private) |
| `-E, --scheduledEndTime` | string |  | Scheduled end time in RFC 3339 format |
| `-S, --scheduledStartTime` | string |  | Scheduled start time in RFC 3339 format **Required.** |
| `-t, --title` | string |  | Title of the live broadcast **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Create a public live broadcast
youtube liveBroadcast insert --title "My Broadcast" --privacyStatus public --scheduledStartTime 2026-01-01T00:00:00Z
# Create a private broadcast with description
youtube liveBroadcast insert --title "Test Stream" --description "Testing" --privacyStatus private --scheduledStartTime 2026-01-01T00:00:00Z
```

### `youtube liveBroadcast insertCuepoint`

Insert a cuepoint into a live broadcast. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-D, --cueDurationSecs` | int | `0` | Cuepoint duration in seconds |
| `-O, --cueInsertionOffsetMs` | int | `0` | Cuepoint insertion offset time in milliseconds |
| `-c, --cueType` | string | `cueTypeAd` | Cue type (cueTypeAd) |
| `-W, --cueWalltimeMs` | uint64 | `0` | Cuepoint wall clock time in milliseconds |
| `-i, --ids` | strings |  | IDs of the broadcasts **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string |  | json\|yaml\|silent |
| `--yes` | bool |  | Confirm the operation |

```bash
# Insert an ad cuepoint of 30 seconds
youtube liveBroadcast insertCuepoint --ids broadcast123 --cueType cueTypeAd --cueDurationSecs 30
# Insert a cuepoint with offset
youtube liveBroadcast insertCuepoint --ids broadcast123 --cueType cueTypeAd --cueInsertionOffsetMs 5000
```

### `youtube liveBroadcast list`

List live broadcasts. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-s, --broadcastStatus` | string |  | Broadcast status filter (all, active, upcoming, completed) |
| `-T, --broadcastType` | string |  | Broadcast type filter (all, event, persistent) |
| `-i, --ids` | strings |  | Return live broadcasts with the given ids |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-m, --mine` | bool |  | List broadcasts owned by the authenticated user |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet,status]` | Comma separated parts |

```bash
# List my live broadcasts
youtube liveBroadcast list --mine
# List live broadcasts by ID
youtube liveBroadcast list --ids broadcast1,broadcast2
# List active live broadcasts
youtube liveBroadcast list --mine --broadcastStatus active --output json
```

### `youtube liveBroadcast transition`

Transition a live broadcast. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-s, --broadcastStatus` | string |  | Broadcast status to transition to (testing, live, complete) **Required.** |
| `-i, --ids` | strings |  | IDs of the broadcasts to transition **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[id,snippet,status]` | Comma separated parts |
| `--yes` | bool |  | Confirm the operation |

```bash
# Transition a broadcast to live
youtube liveBroadcast transition --ids broadcast123 --broadcastStatus live
# End a broadcast
youtube liveBroadcast transition --ids broadcast123 --broadcastStatus complete
```

### `youtube liveBroadcast update`

Update a live broadcast. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d, --description` | string |  | Description of the live broadcast |
| `-i, --id` | strings |  | ID of the live broadcast to update **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-P, --privacyStatus` | string |  | Privacy status (public, unlisted, private) |
| `-E, --scheduledEndTime` | string |  | Scheduled end time in RFC 3339 format |
| `-S, --scheduledStartTime` | string |  | Scheduled start time in RFC 3339 format |
| `-t, --title` | string |  | Title of the live broadcast |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update live broadcast title
youtube liveBroadcast update --id broadcast123 --title "New Title"
# Update privacy status
youtube liveBroadcast update --id broadcast123 --privacyStatus unlisted
```

## `youtube liveChatBan`

Manage YouTube live chat bans.

### `youtube liveChatBan delete`

Delete live chat bans. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the live chat bans to delete **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a live chat ban by ID
youtube liveChatBan delete --ids abc123
# Delete multiple live chat bans
youtube liveChatBan delete --ids abc123,def456
```

### `youtube liveChatBan insert`

Create a live chat ban. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d, --banDurationSeconds` | uint64 | `0` | Duration of the ban in seconds (only for temporary bans) |
| `-t, --banType` | string |  | permanent\|temporary **Required.** |
| `-b, --bannedUserChannelId` | string |  | Channel ID of the user to ban **Required.** |
| `-l, --liveChatId` | string |  | ID of the live chat **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet]` | Parts to include |
| `--yes` | bool |  | Confirm the operation |

```bash
# Ban a user permanently
youtube liveChatBan insert --liveChatId abc123 --bannedUserChannelId UC_xyz --banType permanent
# Ban a user temporarily for 5 minutes
youtube liveChatBan insert --liveChatId abc123 --bannedUserChannelId UC_xyz --banType temporary --banDurationSeconds 300
```

## `youtube liveChatMessage`

Manage YouTube live chat messages.

### `youtube liveChatMessage delete`

Delete live chat messages. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the live chat messages to delete **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a live chat message by ID
youtube liveChatMessage delete --ids abc123
# Delete multiple live chat messages
youtube liveChatMessage delete --ids abc123,def456
```

### `youtube liveChatMessage insert`

Send a live chat message. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --liveChatId` | string |  | ID of the live chat **Required.** |
| `-m, --messageText` | string |  | Text of the message to send **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet]` | Parts to include |
| `--yes` | bool |  | Confirm the operation |

```bash
# Send a message to a live chat
youtube liveChatMessage insert --liveChatId abc123 --messageText "Hello everyone!"
```

### `youtube liveChatMessage list`

List live chat messages. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--hl` | string |  | Localization language for system messages |
| `-l, --liveChatId` | string |  | ID of the live chat **Required.** |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[snippet,authorDetails]` | Comma separated parts |

```bash
# List messages for a live chat
youtube liveChatMessage list --liveChatId abc123
# List messages with limit
youtube liveChatMessage list --liveChatId abc123 --maxResults 10
```

### `youtube liveChatMessage transition`

Transition a live chat message. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the messages to transition **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-s, --status` | string |  | Status to transition to (closed) **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Transition a live chat message to closed
youtube liveChatMessage transition --ids abc123 --status closed
```

## `youtube liveChatModerator`

Manage YouTube live chat moderators.

### `youtube liveChatModerator delete`

Delete live chat moderators. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the live chat moderators to delete **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a live chat moderator by ID
youtube liveChatModerator delete --ids abc123
# Delete multiple live chat moderators
youtube liveChatModerator delete --ids abc123,def456
```

### `youtube liveChatModerator insert`

Add a live chat moderator. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --liveChatId` | string |  | ID of the live chat **Required.** |
| `-m, --moderatorChannelId` | string |  | Channel ID of the moderator **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet]` | Parts to include |
| `--yes` | bool |  | Confirm the operation |

```bash
# Add a moderator to a live chat
youtube liveChatModerator insert --liveChatId abc123 --moderatorChannelId UC_xyz
```

### `youtube liveChatModerator list`

List live chat moderators. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --liveChatId` | string |  | ID of the live chat **Required.** |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[snippet]` | Comma separated parts |

```bash
# List moderators for a live chat
youtube liveChatModerator list --liveChatId abc123
# List moderators with limit
youtube liveChatModerator list --liveChatId abc123 --maxResults 10
```

## `youtube liveStream`

Manage YouTube live streams.

### `youtube liveStream delete`

Delete live streams. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the live streams to delete **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a live stream by ID
youtube liveStream delete --ids stream123
# Delete multiple live streams
youtube liveStream delete --ids stream123,stream456
```

### `youtube liveStream insert`

Create a live stream. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d, --description` | string |  | Description of the live stream |
| `-f, --frameRate` | string |  | Frame rate of the video stream (30fps, 60fps, variable) |
| `-I, --ingestionType` | string |  | Ingestion type (rtmp, dash, webrtc, hls) **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet,cdn,status]` | Comma separated parts |
| `-r, --resolution` | string |  | Resolution of the video stream (240p, 360p, 480p, 720p, 1080p, 1440p, 2160p, variable) **Required.** |
| `-t, --title` | string |  | Title of the live stream **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Create a live stream with RTMP at 1080p
youtube liveStream insert --title "My Stream" --ingestionType rtmp --resolution 1080p --frameRate 60fps
# Create a live stream with description
youtube liveStream insert --title "Gaming Stream" --description "Live gaming session" --ingestionType rtmp --resolution 720p
```

### `youtube liveStream list`

List live streams. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | Return live streams with the given ids |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-m, --mine` | bool |  | List streams owned by the authenticated user |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet,cdn,status]` | Comma separated parts |

```bash
# List my live streams
youtube liveStream list --mine
# List live streams by ID
youtube liveStream list --ids stream1,stream2
# List live streams in JSON format
youtube liveStream list --mine --output json --maxResults 10
```

### `youtube liveStream update`

Update a live stream. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d, --description` | string |  | Description of the live stream |
| `-f, --frameRate` | string |  | Frame rate of the video stream (30fps, 60fps, variable) |
| `-i, --id` | strings |  | ID of the live stream to update **Required.** |
| `-I, --ingestionType` | string |  | Ingestion type (rtmp, dash, webrtc, hls) |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | Channel ID for content owner operations |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-r, --resolution` | string |  | Resolution of the video stream (240p, 360p, 480p, 720p, 1080p, 1440p, 2160p, variable) |
| `-t, --title` | string |  | Title of the live stream |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update live stream title
youtube liveStream update --id stream123 --title "New Title"
# Update live stream CDN settings
youtube liveStream update --id stream123 --resolution 1080p --frameRate 60fps
```

## `youtube member`

Manage YouTube channel members.

### `youtube member list`

List channel members. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-a, --hasAccessToLevel` | string |  | Filter members in the results set to the ones that have access to a level |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-c, --memberChannelId` | string |  | Comma separated list of channel Ids. Only data about members that are part of this list will be included |
| `-m, --mode` | string | `all_current` | listMembersModeUnknown, updates, or all_current |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[snippet]` | Comma separated parts |

```bash
# List all current members
youtube member list --mode all_current
# List members with limit
youtube member list --maxResults 10
```

## `youtube membershipsLevel`

Manage YouTube membership levels.

### `youtube membershipsLevel list`

List membership levels. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |

```bash
# List membership levels in JSON format
youtube membershipsLevel list --output json
```

## `youtube playlist`

Manage YouTube playlists.

### `youtube playlist delete`

Delete playlists. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the playlists to delete **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a playlist by ID
youtube playlist delete --ids PLxxxx
# Delete multiple playlists
youtube playlist delete --ids PLxxx1,PLxxx2
```

### `youtube playlist insert`

Create a playlist. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | Channel id of the playlist **Required.** |
| `-d, --description` | string |  | Description of the playlist |
| `-l, --language` | string |  | Language of the playlist |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --privacy` | string |  | public\|private\|unlisted **Required.** |
| `-a, --tags` | strings |  | Comma separated tags |
| `-t, --title` | string |  | Title of the playlist **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Create a public playlist
youtube playlist insert --title 'My Playlist' --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --privacy public
# Create a private playlist with description
youtube playlist insert --title 'Tutorial Series' --channelId UC_x5X --privacy private --description 'My tutorials'
# Create an unlisted playlist with tags
youtube playlist insert --title 'Music' --channelId UC_x5X --privacy unlisted --tags 'music,pop'
```

### `youtube playlist list`

List playlists. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | Return the playlists owned by the specified channel id |
| `-l, --hl` | string |  | Return content in specified language |
| `-i, --ids` | strings |  | Return the playlists with the given Ids for Stubby or Apiary |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-M, --mine` | bool | `true` | Return the playlists owned by the authenticated user |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | YouTube channel ID linked to the content owner |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet,status]` | Comma separated parts |

```bash
# List my playlists
youtube playlist list --mine
# List playlists by channel ID with limit
youtube playlist list --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --maxResults 10
# List specific playlists in JSON format
youtube playlist list --ids PLxxx1,PLxxx2 --output json
```

### `youtube playlist update`

Update a playlist. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d, --description` | string |  | Description of the playlist |
| `-i, --id` | strings |  | ID of the playlist to update **Required.** |
| `-l, --language` | string |  | Language of the playlist |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --privacy` | string |  | public\|private\|unlisted |
| `-a, --tags` | strings |  | Comma separated tags |
| `-t, --title` | string |  | Title of the playlist |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update playlist title
youtube playlist update --id PLxxx --title 'Updated Title'
# Update playlist description and privacy
youtube playlist update --id PLxxx --description 'New description' --privacy public
# Update playlist tags
youtube playlist update --id PLxxx --tags 'music,pop,2024'
```

## `youtube playlistImage`

Manage YouTube playlist images.

### `youtube playlistImage delete`

Delete playlist images. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the playlist images to delete **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a playlist image by ID
youtube playlistImage delete --ids abc123
# Delete multiple playlist images
youtube playlistImage delete --ids abc123,def456
```

### `youtube playlistImage insert`

Upload a playlist image. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-f, --file` | string |  | Path to the image file **Required.** |
| `-H, --height` | int | `0` | The image height |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | YouTube channel ID linked to the content owner |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --playlistId` | string |  | ID of the playlist this image is associated with **Required.** |
| `-t, --type` | string |  | The image type (e.g., 'hero') |
| `-W, --width` | int | `0` | The image width |
| `--yes` | bool |  | Confirm the operation |

```bash
# Insert a playlist cover image
youtube playlistImage insert --file cover.jpg --playlistId PLxxx
# Insert a hero image
youtube playlistImage insert --file cover.png --playlistId PLxxx --type hero
# Insert an image with custom dimensions
youtube playlistImage insert --file cover.jpg --playlistId PLxxx --width 2048 --height 1152
```

### `youtube playlistImage list`

List playlist images. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | YouTube channel ID linked to the content owner |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-P, --parent` | string |  | Return PlaylistImages for this playlist id |
| `-p, --parts` | strings | `[id,kind,snippet]` | Comma separated parts |

```bash
# List images of a playlist
youtube playlistImage list --parent PLxxx
# List playlist images with limit in JSON format
youtube playlistImage list --parent PLxxx --maxResults 10 --output json
```

### `youtube playlistImage update`

Update a playlist image. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-H, --height` | int | `0` | The image height |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | YouTube channel ID linked to the content owner |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --playlistId` | string |  | ID of the playlist this image is associated with **Required.** |
| `-t, --type` | string |  | The image type (e.g., 'hero') |
| `-W, --width` | int | `0` | The image width |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update a playlist image
youtube playlistImage update --playlistId PLxxx --type hero --width 2048 --height 1152
```

## `youtube playlistItem`

Manage YouTube playlist items.

### `youtube playlistItem delete`

Delete playlist items. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the playlist items to delete **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a playlist item by ID
youtube playlistItem delete --ids abc123
# Delete multiple playlist items
youtube playlistItem delete --ids abc123,def456
```

### `youtube playlistItem insert`

Add a playlist item. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | ID that YouTube uses to uniquely identify the user that added the item to the playlist **Required.** |
| `-d, --description` | string |  | Description of the playlist item |
| `-C, --kChannelId` | string |  | ID of the channel if kind is channel |
| `-Y, --kPlaylistId` | string |  | ID of the playlist if kind is playlist |
| `-V, --kVideoId` | string |  | ID of the video if kind is video |
| `-k, --kind` | string |  | video\|channel\|playlist **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-y, --playlistId` | string |  | The id that YouTube uses to uniquely identify the playlist that the item is in **Required.** |
| `-p, --privacy` | string |  | public\|private\|unlisted |
| `-t, --title` | string |  | Title of the playlist item |
| `--yes` | bool |  | Confirm the operation |

```bash
# Add a video to a playlist
youtube playlistItem insert --kind video --playlistId PLxxx --channelId UC_x5X --kVideoId dQw4w9WgXcQ
# Add a video to a playlist with privacy setting
youtube playlistItem insert --kind video --playlistId PLxxx --channelId UC_x5X --kVideoId dQw4w9 --privacy public
```

### `youtube playlistItem list`

List playlist items. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the playlist items to list |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet,status]` | Comma separated parts |
| `-y, --playlistId` | string |  | Return the playlist items within the given playlist |
| `-v, --videoId` | string |  | Return the playlist items associated with the given video id |

```bash
# List items in a playlist
youtube playlistItem list --playlistId PLxxx
# List playlist items with limit in JSON format
youtube playlistItem list --playlistId PLxxx --maxResults 20 --output json
# List specific playlist items by IDs
youtube playlistItem list --ids abc123,def456
```

### `youtube playlistItem update`

Update a playlist item. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-d, --description` | string |  | Description of the playlist item |
| `-i, --id` | strings |  | ID of the playlist item to update **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --privacy` | string |  | public\|private\|unlisted |
| `-t, --title` | string |  | Title of the playlist item |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update playlist item title
youtube playlistItem update --id abc123 --title 'Updated Title'
# Update playlist item description and privacy
youtube playlistItem update --id abc123 --description 'New description' --privacy public
# Update playlist item privacy with JSON output
youtube playlistItem update --id abc123 --privacy private --output json
```

## `youtube search`

Manage YouTube search.

### `youtube search list`

Search resources. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--channelId` | string |  | Filter on resources belonging to this channelId |
| `--channelType` | string | `channelTypeUnspecified` | channelTypeUnspecified\|any\|show |
| `--eventType` | string | `none` | none\|upcoming\|live\|completed |
| `--for` | string |  | mine\|developer\|contentOwner |
| `--location` | string |  | Filter on location of the video |
| `--locationRadius` | string |  | Filter on distance from the location |
| `--maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `--onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `--order` | string | `relevance` | searchSortUnspecified\|date\|rating\|viewCount\|relevance\|title\|videoCount |
| `--output` | string | `table` | json\|yaml\|table |
| `--parts` | strings | `[id,snippet]` | Comma separated parts |
| `--publishedAfter` | string |  | Filter on resources published after this date |
| `--publishedBefore` | string |  | Filter on resources published before this date |
| `--q` | string |  | Textual search terms to match |
| `--regionCode` | string |  | Display the content as seen by viewers in this country |
| `--relevanceLanguage` | string |  | Return results relevant to this language |
| `--safeSearch` | string | `moderate` | safeSearchSettingUnspecified\|none\|moderate\|strict |
| `--topicId` | string |  | Restrict results to a particular topic |
| `--types` | strings |  | Restrict results to a particular set of resource types from One Platform |
| `--videoCaption` | string | `any` | videoCaptionUnspecified\|any\|closedCaption\|none |
| `--videoCategoryId` | string |  | Filter on videos in a specific category |
| `--videoDefinition` | string |  | Filter on the definition of the videos |
| `--videoDimension` | string | `any` | any\|2d\|3d |
| `--videoDuration` | string | `any` | videoDurationUnspecified\|any\|short\|medium\|long |
| `--videoEmbeddable` | string |  | videoEmbeddableUnspecified\|any\|true |
| `--videoLicense` | string |  | any\|youtube\|creativeCommon |
| `--videoPaidProductPlacement` | string |  | videoPaidProductPlacementUnspecified\|any\|true |
| `--videoSyndicated` | string |  | videoSyndicatedUnspecified\|any\|true |
| `--videoType` | string |  | videoTypeUnspecified\|any\|movie\|episode |

```bash
# Search for videos by keyword
youtube search list --q 'golang tutorial' --maxResults 10
# Search videos by duration in JSON format
youtube search list --q 'music' --types video --videoDuration medium --output json
# List latest videos from a channel
youtube search list --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --order date --maxResults 5
# Search for live streams
youtube search list --q 'live coding' --eventType live --types video
```

## `youtube subscription`

Manage YouTube subscriptions.

### `youtube subscription delete`

Delete subscriptions. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the subscriptions to delete **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a subscription by ID
youtube subscription delete --ids abc123
# Delete multiple subscriptions
youtube subscription delete --ids abc123,def456
```

### `youtube subscription insert`

Subscribe to a channel. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | ID of the channel to subscribe to **Required.** |
| `-d, --description` | string |  | Description of the subscription |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-s, --subscriberChannelId` | string |  | Subscriber's channel id **Required.** |
| `-t, --title` | string |  | Title of the subscription |
| `--yes` | bool |  | Confirm the operation |

```bash
# Subscribe to a channel
youtube subscription insert --subscriberChannelId UC_abc --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw
# Subscribe with a title
youtube subscription insert --subscriberChannelId UC_abc --channelId UC_x5X --title 'Google Developers'
```

### `youtube subscription list`

List subscriptions. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | Return the subscriptions of the given channel owner |
| `-F, --for` | string |  | mine\|myRecentSubscribers\|mySubscribers |
| `-C, --forChannelId` | string |  | Return the subscriptions to the subset of these channels that the authenticated user is subscribed to |
| `-i, --ids` | strings |  | Return the subscriptions with the given ids for Stubby or Apiary |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | YouTube channel ID linked to the content owner |
| `-O, --order` | string | `relevance` | subscriptionOrderUnspecified\|relevance\|unread\|alphabetical |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |

```bash
# List my subscriptions
youtube subscription list --for mine
# List subscriptions by channel ID with limit
youtube subscription list --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --maxResults 10
# List specific subscriptions in JSON format
youtube subscription list --ids abc123,def456 --output json
# List subscriptions for a channel alphabetically
youtube subscription list --forChannelId UC_x5XG1OV2P6uZZ5FSM9Ttw --order alphabetical
```

## `youtube superChatEvent`

Manage YouTube Super Chat events.

### `youtube superChatEvent list`

List Super Chat events. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --hl` | string |  | Return rendered funding amounts in specified language |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |

```bash
# List Super Chat events
youtube superChatEvent list --maxResults 10
```

## `youtube thirdPartyLink`

Manage YouTube third-party links.

### `youtube thirdPartyLink delete`

Delete third-party links. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-e, --externalChannelId` | string |  | Channel ID to which changes should be applied, for delegation |
| `-l, --linkingToken` | string |  | Linking token that identifies the YouTube account and channel **Required.** |
| `-t, --type` | string |  | Type of the link: linkUnspecified\|channelToStoreLink **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a third-party link
youtube thirdPartyLink delete --linkingToken abc123 --type channelToStoreLink
```

### `youtube thirdPartyLink insert`

Create a third-party link. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-e, --externalChannelId` | string |  | Channel ID to which changes should be applied, for delegation |
| `-s, --linkStatus` | string |  | Status of the link: unknown\|failed\|pending\|linked |
| `-l, --linkingToken` | string |  | Linking token that identifies the YouTube account and channel **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet,status]` | Comma separated parts |
| `-t, --type` | string |  | Type of the link: linkUnspecified\|channelToStoreLink **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Insert a new third-party link
youtube thirdPartyLink insert --linkingToken abc123 --type channelToStoreLink --linkStatus pending --parts snippet,status
```

### `youtube thirdPartyLink list`

List third-party links. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-e, --externalChannelId` | string |  | Channel ID to which changes should be applied, for delegation |
| `-l, --linkingToken` | string |  | Linking token that identifies the YouTube account and channel |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[snippet,status]` | Comma separated parts |
| `-t, --type` | string |  | Type of the link: linkUnspecified\|channelToStoreLink |

```bash
# List all third-party links
youtube thirdPartyLink list --parts snippet,status
# List links by linking token
youtube thirdPartyLink list --linkingToken abc123 --parts snippet,status
# List links by type
youtube thirdPartyLink list --type channelToStoreLink --parts snippet,status
```

### `youtube thirdPartyLink update`

Update a third-party link. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-e, --externalChannelId` | string |  | Channel ID to which changes should be applied, for delegation |
| `-s, --linkStatus` | string |  | Status of the link: unknown\|failed\|pending\|linked |
| `-l, --linkingToken` | string |  | Linking token that identifies the YouTube account and channel **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-p, --parts` | strings | `[snippet,status]` | Comma separated parts |
| `-t, --type` | string |  | Type of the link: linkUnspecified\|channelToStoreLink |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update a third-party link status
youtube thirdPartyLink update --linkingToken abc123 --linkStatus linked --parts snippet,status
# Update a third-party link type
youtube thirdPartyLink update --linkingToken abc123 --type channelToStoreLink --parts snippet,status
```

## `youtube thumbnail`

Manage YouTube video thumbnails.

### `youtube thumbnail set`

Set a thumbnail for a video. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-f, --file` | string |  | Path to the thumbnail file **Required.** |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-v, --videoId` | string |  | ID of the video **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Set a thumbnail for a video
youtube thumbnail set --file image.jpg --videoId dQw4w9WgXcQ
# Set a thumbnail with JSON output
youtube thumbnail set --file image.png --videoId dQw4w9WgXcQ --output json
```

## `youtube video`

Manage YouTube videos.

### `youtube video delete`

Delete videos. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the videos to delete **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Delete a video by ID
youtube video delete --ids dQw4w9WgXcQ
# Delete multiple videos
youtube video delete --ids dQw4w9WgXcQ,abc123
```

### `youtube video getRating`

Get video ratings. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the videos to get the rating for **Required.** |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string | `table` | json\|yaml\|table |

```bash
# Get rating of a video
youtube video getRating --ids dQw4w9WgXcQ
# Get ratings of multiple videos in JSON format
youtube video getRating --ids dQw4w9WgXcQ,abc123 --output json
```

### `youtube video insert`

Upload a video. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-A, --autoLevels` | bool | `true` | Should auto-levels be applied to the upload |
| `-g, --categoryId` | string |  | Category of the video **Required.** |
| `-c, --channelId` | string |  | Channel id of the video |
| `-M, --containsSyntheticMedia` | bool |  | Whether the video contains altered or synthetic media |
| `-d, --description` | string |  | Description of the video |
| `-E, --embeddable` | bool | `true` | Whether the video is embeddable |
| `-f, --file` | string |  | Path to the video file **Required.** |
| `-K, --forKids` | bool |  | Whether the video is for kids |
| `-l, --language` | string |  | Language of the video |
| `-L, --license` | string |  | youtube\|creativeCommon |
| `-N, --notifySubscribers` | bool | `true` | Notify the channel subscribers about the new video |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-B, --onBehalfOfContentOwnerChannel` | string |  | YouTube channel ID linked to the content owner |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-y, --playlistId` | string |  | Playlist id of the video |
| `-p, --privacy` | string |  | public\|private\|unlisted **Required.** |
| `-P, --publicStatsViewable` | bool |  | Whether the extended video statistics can be viewed by everyone |
| `-U, --publishAt` | string |  | Datetime when the video is scheduled to publish |
| `-D, --recordingDate` | string |  | Date and time when the video was recorded |
| `-S, --stabilize` | bool | `true` | Should stabilize be applied to the upload |
| `-a, --tags` | strings |  | Comma separated tags |
| `-u, --thumbnail` | string |  | Path to the thumbnail file |
| `-t, --title` | string |  | Title of the video |
| `--yes` | bool |  | Confirm the operation |

```bash
# Upload a public video
youtube video insert --file video.mp4 --title 'My Video' --categoryId 22 --privacy public
# Upload a private video with tags
youtube video insert --file video.mp4 --title 'Tutorial' --categoryId 27 --privacy private --tags 'go,tutorial'
# Upload an unlisted video with custom thumbnail
youtube video insert --file video.mp4 --title 'Music Video' --categoryId 10 --privacy unlisted --thumbnail cover.jpg
```

### `youtube video list`

List videos. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --chart` | string |  | chartUnspecified\|mostPopular |
| `-l, --hl` | string |  | Specifies the localization language |
| `-i, --ids` | strings |  | Return videos with the given ids |
| `-L, --locale` | string |  | Locale of the video metadata to return |
| `-H, --maxHeight` | int | `0` | Max height of the embedded player in pixels |
| `-n, --maxResults` | int | `5` | The maximum number of items that should be returned, 0 for no limit |
| `-W, --maxWidth` | int | `0` | Max width of the embedded player in pixels |
| `-R, --myRating` | string |  | Return videos liked/disliked by the authenticated user |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet,status,statistics]` | Comma separated parts |
| `-r, --regionCode` | string |  | Specific to the specified region |
| `-g, --videoCategoryId` | string |  | Category of the video |

```bash
# List a video by ID
youtube video list --ids IBju0NwjQRc
# List multiple videos in JSON format
youtube video list --ids IBju0NwjQRc,wZ9z3FUuNvQ --output json
# List most popular videos by region
youtube video list --chart mostPopular --regionCode US --maxResults 10
# List my liked videos
youtube video list --myRating like --output yaml
```

### `youtube video rate`

Rate a video. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-i, --ids` | strings |  | IDs of the videos to rate **Required.** |
| `-r, --rating` | string |  | like\|dislike\|none **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Like a video
youtube video rate --ids dQw4w9WgXcQ --rating like
# Dislike multiple videos
youtube video rate --ids dQw4w9WgXcQ,abc123 --rating dislike
# Remove rating from a video
youtube video rate --ids dQw4w9WgXcQ --rating none
```

### `youtube video reportAbuse`

Report abuse on a video. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --comments` | string |  | Additional comments regarding the abuse report |
| `-i, --ids` | strings |  | IDs of the videos to report abuse on **Required.** |
| `-l, --language` | string |  | Language that the content was viewed in |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `-r, --reasonId` | string |  | ID of the reason for reporting abuse **Required.** |
| `-s, --secondaryReasonId` | string |  | ID of the secondary reason for reporting abuse |
| `--yes` | bool |  | Confirm the operation |

```bash
# Report abuse on a video
youtube video reportAbuse --ids dQw4w9WgXcQ --reasonId V
# Report abuse with secondary reason and language
youtube video reportAbuse --ids dQw4w9WgXcQ --reasonId V --secondaryReasonId V1 --language en
# Report abuse with comments
youtube video reportAbuse --ids dQw4w9WgXcQ --reasonId N --comments 'Spam content'
```

### `youtube video update`

Update a video. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-g, --categoryId` | string |  | Category of the video |
| `-M, --containsSyntheticMedia` | bool |  | Whether the video contains altered or synthetic media |
| `-d, --description` | string |  | Description of the video |
| `-E, --embeddable` | bool | `true` | Whether the video is embeddable |
| `-i, --id` | strings |  | ID of the video to update **Required.** |
| `-l, --language` | string |  | Language of the video |
| `-L, --license` | string |  | youtube\|creativeCommon |
| `-o, --output` | string |  | json\|yaml\|silent |
| `-y, --playlistId` | string |  | Playlist id of the video |
| `-p, --privacy` | string |  | public\|private\|unlisted |
| `-U, --publishAt` | string |  | Datetime when the video is scheduled to publish |
| `-D, --recordingDate` | string |  | Date and time when the video was recorded |
| `-a, --tags` | strings |  | Comma separated tags |
| `-u, --thumbnail` | string |  | Path to the thumbnail file |
| `-t, --title` | string |  | Title of the video |
| `--yes` | bool |  | Confirm the operation |

```bash
# Update video title
youtube video update --id dQw4w9WgXcQ --title 'New Title'
# Update video description and privacy
youtube video update --id dQw4w9WgXcQ --description 'Updated description' --privacy public
# Schedule a private video to publish later
youtube video update --id dQw4w9WgXcQ --privacy private --publishAt 2026-08-18T15:00:00Z
# Update video tags and category
youtube video update --id dQw4w9WgXcQ --tags 'music,pop,2024' --categoryId 10
```

## `youtube videoAbuseReportReason`

Manage YouTube video abuse report reasons.

### `youtube videoAbuseReportReason list`

List video abuse report reasons. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --hl` | string |  | Host language |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |

```bash
# List video abuse report reasons
youtube videoAbuseReportReason list --hl en
```

## `youtube videoCategory`

Manage YouTube video categories.

### `youtube videoCategory list`

List video categories. Read only.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-l, --hl` | string |  | Host language |
| `-i, --ids` | strings |  | IDs of the video categories |
| `-o, --output` | string | `table` | json\|yaml\|table |
| `-p, --parts` | strings | `[id,snippet]` | Comma separated parts |
| `-r, --regionCode` | string | `US` | Region code |

## `youtube watermark`

Manage YouTube watermarks.

### `youtube watermark set`

Set a channel watermark. Changes the channel: needs the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | ID of the channel **Required.** |
| `-d, --durationMs` | uint64 | `0` | Duration in milliseconds for which the watermark should be displayed |
| `-f, --file` | string |  | Path to the watermark file **Required.** |
| `-p, --inVideoPosition` | string |  | topLeft\|topRight\|bottomLeft\|bottomRight |
| `-m, --offsetMs` | uint64 | `0` | Defines the time at which the watermark will appear |
| `-t, --offsetType` | string |  | offsetFromStart\|offsetFromEnd |
| `-b, --onBehalfOfContentOwner` | string |  | ID of the content owner, for YouTube content partners |
| `--yes` | bool |  | Confirm the operation |

```bash
# Set a watermark for a channel
youtube watermark set --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw --file watermark.png
# Set a watermark with position and offset
youtube watermark set --channelId UC_x5X --file logo.png --inVideoPosition bottomRight --offsetType offsetFromEnd --offsetMs 1000
# Set a watermark with duration
youtube watermark set --channelId UC_x5X --file logo.png --durationMs 5000 --offsetMs 2000
```

### `youtube watermark unset`

Unset a channel watermark. Irreversible: list what goes, get the user's yes, then `--yes`.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-c, --channelId` | string |  | ID of the channel **Required.** |
| `--yes` | bool |  | Confirm the operation |

```bash
# Unset watermark for a channel
youtube watermark unset --channelId UC_x5XG1OV2P6uZZ5FSM9Ttw
```

## Program commands

### `youtube agent`

Start an agent to automate YouTube workflows.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--api-key` | string |  | API key for the model provider |
| `-a, --args` | string | `console` | Launcher arguments as a single string |
| `--base-url` | string |  | Base URL for the model provider's API endpoint |
| `-i, --instruction` | string |  | Override the built-in agent instruction |
| `-m, --model` | string | `gemini-3.8-flash` | Model name |
| `-p, --provider` | string | `google` | LLM provider: google for Gemini(compatible), openai for OpenAI(compatible), claude-code or codex to use that CLI's own sign-in |

```bash
# Gemini(compatible) (default)
youtube agent --provider google --model gemini-3.8-flash --api-key "YOUR_KEY"
youtube agent --provider google --model gemini-3.8-flash --api-key "YOUR_KEY" --base-url "https://custom.endpoint.com/"
# OpenAI(compatible) (OpenAI, DeepSeek, Ollama, vLLM, etc.)
youtube agent --provider openai --model gpt-5.6-terra --api-key "YOUR_KEY"
youtube agent --provider openai --model deepseek-flash --api-key "YOUR_KEY" --base-url "https://api.deepseek.com/"
# Claude Code or Codex CLI, signed in already
youtube agent --provider claude-code
youtube agent --provider codex
```

### `youtube auth`

Authenticate with YouTube APIs.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-t, --cacheToken` | string | `youtube.token.json` | Path to token cache file, or Base64 encoded string, or JSON string (env: YOUTUBE_CLI_CACHE_TOKEN) |
| `-c, --credential` | string | `client_secret.json` | Path to client secret file, or Base64 encoded string, or JSON string (env: YOUTUBE_CLI_CREDENTIAL) |

### `youtube help`

Help about any command.


### `youtube mcp`

Start MCP server.

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-m, --mode` | string | `stdio` | stdio\|http |

```bash
# Start MCP server in stdio mode (default)
youtube mcp

# Start MCP server in HTTP mode at http://localhost:8216/mcp
# Requires OAuth (Google account) for all MCP tool calls
youtube mcp --mode http
```

### `youtube version`

Show the version of youtube.

