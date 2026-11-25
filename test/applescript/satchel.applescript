property contactName : "Real Satchel"
property timeoutSeconds : 90
property chatDb : "~/Library/Messages/chat.db"
property likeTapback : 2001
property evidenceDir : "/tmp/satchel-verify"

on allCases()
	return {¬
		{"reset", "reset", "text", "Satchel"}, ¬
		{"chat", "hi", "text", ""}, ¬
		{"save-link-scheme", "https://www.rust-lang.org/learn", "tapback", ""}, ¬
		{"save-link-bare", "github.com/rust-lang/rust", "tapback", ""}, ¬
		{"save-link-with-comment", "check this place out https://www.zillow.com/homes/austin looks nice", "tapback", ""}, ¬
		{"save-two-links-prefix", "tour at example.org, details at example.org/tour", "tapback", ""}, ¬
		{"save-link-duplicate", "https://www.rust-lang.org/learn", "tapback", ""}, ¬
		{"recall-link", "what rust link did i save?", "text-only", "rust-lang.org"}, ¬
		{"suspicious-not-link", "node.js is my favorite runtime", "text-only", ""}, ¬
		{"file-like-not-link", "the readme.md has the setup steps", "text-only", ""}, ¬
		{"remember-note", "remember my gate code is 4521", "text-only", ""}, ¬
		{"remember-note-emoji", "remember the 🍜 pho place on 5th street is the best", "text-only", ""}, ¬
		{"recall-note", "what's my gate code?", "text-only", "4521"}, ¬
		{"burst-note-correct", "remember my locker code is 1234||wait no, it's 5678", "text-only", ""}, ¬
		{"recall-burst-correct", "what's my locker code?", "text-only", "5678"}, ¬
		{"burst-note-add", "remember the wifi password is otter42||that's for the cabin", "text-only", ""}, ¬
		{"recall-burst-add", "what's the cabin wifi password?", "text-only", "otter42"}, ¬
		{"burst-unrelated-notes", "remember my passport number ends in 4417||also remember mom's birthday is june 3", "text-only", ""}, ¬
		{"recall-unrelated-1", "what does my passport number end in?", "text-only", "4417"}, ¬
		{"recall-unrelated-2", "when is mom's birthday?", "text-only", "june 3"}, ¬
		{"burst-note-and-link", "remember the plumber is joe at 555-0199||https://github.com/cloudflare/workers-rs", "text", ""}, ¬
		{"recall-burst-note", "who's the plumber?", "text-only", "joe"}, ¬
		{"recall-burst-link", "what github repo did i save?", "text-only", "workers-rs"}, ¬
		{"note-then-correct", "remember my bike lock code is 2468", "text-only", ""}, ¬
		{"correct-after", "actually it's 1357", "text", ""}, ¬
		{"recall-after-correct", "what's my bike lock code?", "text-only", "1357"}, ¬
		{"note-then-add", "remember my dentist is dr. kim", "text-only", ""}, ¬
		{"add-after", "she's on 5th street", "text", ""}, ¬
		{"recall-after-add", "where's my dentist?", "text-only", "5th"}, ¬
		{"save-image", "file:~/Pictures/satchel-test/sample-photo.jpg", "tapback", ""}, ¬
		{"save-video", "file:~/Pictures/satchel-test/sample-video.mp4", "tapback", ""}, ¬
		{"recall-image", "send me that picture", "attachment", ""}, ¬
		{"recall-video", "send me that video", "attachment", ""}, ¬
		{"save-voice", "file:~/Pictures/satchel-test/sample-voice.m4a", "tapback", ""}, ¬
		{"save-heic", "file:~/Pictures/satchel-test/sample-photo.heic", "tapback", ""}, ¬
		{"save-voice-caf", "file:~/Pictures/satchel-test/sample-voice.caf", "tapback", ""}, ¬
		{"save-contact-photo", "file:~/Pictures/satchel-test/sample-contact-photo.vcf", "tapback", ""}, ¬
		{"save-docx", "file:~/Pictures/satchel-test/sample-lease.docx", "tapback", ""}, ¬
		{"save-tweet", "https://x.com/jack/status/20", "tapback", ""}, ¬
		{"burst-link-words", "https://www.allrecipes.com/recipe/158968/spinach-and-feta-turkey-burgers/||for sunday dinner", "tapback", ""}, ¬
		{"search-recipe", "send me that turkey burger recipe again", "text", "allrecipes.com"}, ¬
		{"pile-save", "https://www.zillow.com/homes/austin_rb/ cute 2 bed near the lake", "tapback", ""}, ¬
		{"pile-chat-1", "lol my cat just knocked my coffee over", "text-only", ""}, ¬
		{"pile-chat-2", "remember my sister's flight lands at 6", "text-only", ""}, ¬
		{"pile-chat-3", "ok thanks", "text-only", ""}, ¬
		{"pile-ask", "wait what was that apartment place by the lake i sent", "text", "zillow.com"}, ¬
		{"search-today", "what did i save today?", "text", ""}, ¬
		{"search-list", "send me the stuff i saved yesterday", "text", ""}, ¬
		{"search-pick", "#2", "text", ""}, ¬
		{"search-missing", "send me that pottery class link", "text-only", ""}, ¬
		{"s-save-video", "https://www.youtube.com/watch?v=4BOTvaRaDjI hip stretches my PT sent", "tapback", ""}, ¬
		{"s-save-note", "remember the cabin wifi password is otter42", "text-only", ""}, ¬
		{"s-save-bread", "https://en.wikipedia.org/wiki/Sourdough want to try baking this", "tapback", ""}, ¬
		{"s-save-repo", "https://github.com/tokio-rs/tokio for the rust side project", "tapback", ""}, ¬
		{"s-chat-1", "lol today was so long", "text-only", ""}, ¬
		{"s-chat-2", "who won the world cup in 2018", "text-only", ""}, ¬
		{"s-find-video", "what was that stretching video", "text", "youtube.com"}, ¬
		{"s-find-bread", "send me the bread thing i wanted to bake", "text", "wikipedia.org"}, ¬
		{"s-find-note", "whats the internet password at the cabin", "text-only", "otter42"}, ¬
		{"s-find-repo", "that async rust library i saved", "text", "github.com"}, ¬
		{"s-find-missing", "send me that pasta recipe", "text-only", ""}, ¬
		{"s-list", "what have i saved so far", "text", ""}, ¬
		{"s-pick-after-list", "send me the sourdough one", "text", "wikipedia.org"}, ¬
		{"burst-link-off-topic", "https://en.wikipedia.org/wiki/Bagel||what's the weather in sf today?", "both", ""}, ¬
		{"burst-two-links-one-asked", "https://github.com/cloudflare/workers-rs||https://example.org/sketchy-deal is this one a scam?", "both", ""}, ¬
		{"burst-messy", "whats the capital of peru||https://en.wikipedia.org/wiki/Lima||also save this pic||file:~/Pictures/satchel-test/sample-photo.jpg", "both", "lima"}, ¬
		{"file-question-not-saved", "file:~/Pictures/satchel-test/sample-photo.jpg||what's in this picture?", "text-only", ""}, ¬
		{"burst-link-small-talk", "https://en.wikipedia.org/wiki/Sea_otter||lol", "tapback", ""}, ¬
		{"link-then-words", "https://letterboxd.com/film/parasite-2019/", "tapback", ""}, ¬
		{"words-after", "that's the movie for friday night", "tapback", ""}, ¬
		{"save-tiktok-asks", "https://www.tiktok.com/@scout2015/video/6718335390845095173", "both", "about"}, ¬
		{"tiktok-answer", "it's a cute dog video", "tapback", ""}, ¬
		{"recall-voice", "send me that voice memo", "attachment", ""}, ¬
		{"save-contact", "file:~/Pictures/satchel-test/sample-contact.vcf", "tapback", ""}, ¬
		{"recall-contact", "send me that contact", "attachment", ""}, ¬
		{"save-location", "file:~/Pictures/satchel-test/sample-pin.loc.vcf", "tapback", ""}, ¬
		{"recall-location", "send me that location", "attachment", ""}, ¬
		{"save-file", "file:~/Pictures/satchel-test/sample-lease.pdf", "tapback", ""}, ¬
		{"recall-file", "send me that pdf", "attachment", ""}, ¬
		{"question-not-saved", "send me the chili recipe", "text-only", ""}, ¬
		{"link-question-not-saved", "is this site legit? https://example.net/deal", "text-only", ""}, ¬
		{"save-after-asking", "ok save it", "tapback", ""}, ¬
		{"save-and-answer", "save https://doc.rust-lang.org/book and tell me what it is", "text", "book"}, ¬
		{"recall-after-answer", "what was that book link?", "text-only", "doc.rust-lang.org"}, ¬
		{"remind-in", "remind me in 1 minute to drink water", "text-only", ""}, ¬
		{"remind-asks-zone", "remind me tomorrow at 9 am to stretch", "text-only", "zone"}, ¬
		{"remind-set", "pst", "text-only", ""}, ¬
		{"remind-correct", "no, make it 10 am", "text-only", ""}, ¬
		{"remind-change-later", "change my stretch reminder to 11 am", "text-only", ""}, ¬
		{"remind-cancel", "cancel my stretch reminder", "text-only", ""}, ¬
		{"zone-move", "i moved to nyc", "text-only", ""}, ¬
		{"zone-move-back", "actually i moved back to LA", "text-only", ""}, ¬
		{"remind-fires", "", "text-only", "water"}}
end allCases

on run argv
	set satchelNumber to my lookupNumber()
	if argv is not {} and item 1 of argv is "doctor" then return my doctor(satchelNumber)
	if argv is not {} and item 1 of argv is "list" then
		set names to {}
		repeat with c in my allCases()
			set end of names to item 1 of c
		end repeat
		set AppleScript's text item delimiters to linefeed
		return names as text
	end if

	do shell script "mkdir -p " & quoted form of evidenceDir
	set evidencePath to evidenceDir & "/" & (do shell script "date +%Y%m%d-%H%M%S") & ".txt"
	set results to {}
	set failures to 0
	repeat with c in my allCases()
		set {caseName, caseText, wantKind, wantText} to contents of c
		if argv is {} or argv contains caseName then
			set {passed, replies} to my runCase(satchelNumber, caseText, wantKind, wantText)
			if passed then
				set status to "PASS"
			else
				set status to "FAIL"
				set failures to failures + 1
			end if
			set end of results to status & "  " & caseName
			my writeEvidence(evidencePath, status & "  " & caseName & linefeed & "  sent: " & caseText & linefeed & "  want: " & wantKind & " " & wantText & linefeed & "  got:  " & replies)
		end if
	end repeat

	set AppleScript's text item delimiters to linefeed
	set report to (results as text) & linefeed & "evidence: " & evidencePath
	if failures > 0 then error report number failures
	return report
end run

on lookupNumber()
	tell application "Contacts" to set rawNumber to value of first phone of (first person whose name is contactName)
	return do shell script "echo " & quoted form of rawNumber & " | tr -d ' ()-'"
end lookupNumber

on doctor(satchelNumber)
	set checks to {"contact: found"}
	try
		do shell script "sqlite3 -readonly " & chatDb & " 'SELECT 1'"
		set end of checks to "chat.db: readable"
	on error
		set end of checks to "chat.db: BLOCKED (give the app running osascript Full Disk Access)"
	end try
	tell application "Messages" to set accountCount to count of (accounts whose service type = iMessage and enabled is true)
	set end of checks to "imessage accounts: " & accountCount
	set end of checks to "worker: " & (do shell script "cd " & quoted form of my repoRoot() & " && npx -y wrangler@4 deployments status 2>&1 | grep 'Version(s)' | sed 's/ *Version(s): *//'")
	set AppleScript's text item delimiters to linefeed
	return checks as text
end doctor

on runCase(satchelNumber, caseText, wantKind, wantText)
	set startDate to do shell script "echo $(( ($(date +%s) - 978307200) * 1000000000 ))"
	if caseText is not "" then
		-- "first||second" sends two texts one second apart, inside one debounce window.
		-- A part that starts with "file:~/" sends that file.
		set AppleScript's text item delimiters to "||"
		set toSend to {}
		repeat with part in text items of caseText
			set part to part as text
			if part starts with "file:~/" then set part to POSIX file ((POSIX path of (path to home folder)) & (text 8 thru -1 of part))
			set end of toSend to part
		end repeat
		tell application "Messages"
			set toSatchel to participant satchelNumber of (first account whose service type = iMessage)
			repeat with i from 1 to count of toSend
				if i > 1 then delay 1
				send (item i of toSend) to toSatchel
			end repeat
		end tell
	end if
	set fromSatchel to "FROM message m JOIN handle h ON m.handle_id = h.ROWID WHERE h.id = '" & satchelNumber & "' AND m.is_from_me = 0 AND m.date > " & startDate
	-- Count only a tapback on the message this case sent. A late tapback on an earlier message must not pass it.
	-- Outgoing rows can have handle_id 0. Find the sent message through the Satchel chat.
	set sentGuid to "(SELECT s.guid FROM message s JOIN chat_message_join cm ON cm.message_id = s.ROWID JOIN chat_handle_join ch ON ch.chat_id = cm.chat_id WHERE ch.handle_id = h.ROWID AND s.is_from_me = 1 AND s.date > " & startDate & " ORDER BY s.ROWID DESC LIMIT 1)"
	set countQuery to "SELECT coalesce(sum(m.associated_message_type = " & likeTapback & " AND m.associated_message_guid LIKE '%' || " & sentGuid & "), 0) || '|' || " & ¬
		"coalesce(sum(m.associated_message_type = 0 AND (instr(lower(coalesce(m.text, '')), lower('" & wantText & "')) > 0" & ¬
		" OR instr(hex(m.attributedBody), hex('" & wantText & "')) > 0)), 0) || '|' || " & ¬
		"coalesce(sum(m.cache_has_attachments), 0) " & fromSatchel
	set passed to false
	repeat (timeoutSeconds div 3) times
		delay 3
		set counts to do shell script "sqlite3 -readonly " & chatDb & " " & quoted form of countQuery
		set AppleScript's text item delimiters to "|"
		set likes to (text item 1 of counts) as integer
		set matches to (text item 2 of counts) as integer
		set attachments to (text item 3 of counts) as integer
		if wantKind is "tapback" and likes > 0 then set passed to true
		if wantKind is "attachment" and attachments > 0 then set passed to true
		if wantKind is "text" and matches > 0 then set passed to true
		if wantKind is "text-only" and matches > 0 then set passed to (likes = 0)
		if wantKind is "both" and matches > 0 and likes > 0 then set passed to true
		if passed or (wantKind is "text-only" and matches > 0) then exit repeat
	end repeat
	set repliesQuery to "SELECT group_concat(CASE WHEN m.associated_message_type = " & likeTapback & ¬
		" THEN '[like]' WHEN m.cache_has_attachments THEN '[attachment]' ELSE coalesce(m.text, '[attributed text]') END, ' | ') " & fromSatchel
	set replies to do shell script "sqlite3 -readonly " & chatDb & " " & quoted form of repliesQuery
	if replies is "" then set replies to "(no reply in " & timeoutSeconds & "s)"
	return {passed, replies}
end runCase

on repoRoot()
	return do shell script "cd " & quoted form of POSIX path of ((path to me as text) & "::") & " && git rev-parse --show-toplevel"
end repoRoot

on writeEvidence(evidencePath, entry)
	do shell script "printf '%s\\n' " & quoted form of entry & " >> " & quoted form of evidencePath
end writeEvidence
