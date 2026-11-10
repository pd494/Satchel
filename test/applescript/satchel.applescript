property contactName : "Real Satchel"
property timeoutSeconds : 90
property chatDb : "~/Library/Messages/chat.db"
property likeTapback : 2001
property evidenceDir : "/tmp/satchel-verify"

on allCases()
	return {¬
		{"chat", "hi", "text", ""}, ¬
		{"save-link-scheme", "https://www.rust-lang.org/learn", "tapback", ""}, ¬
		{"save-link-bare", "example.com/apartments", "tapback", ""}, ¬
		{"save-link-with-comment", "check this place out https://www.zillow.com/homes/austin looks nice", "tapback", ""}, ¬
		{"save-two-links-prefix", "tour at example.org, details at example.org/tour", "tapback", ""}, ¬
		{"save-link-duplicate", "https://www.rust-lang.org/learn", "tapback", ""}, ¬
		{"suspicious-not-link", "node.js is my favorite runtime", "text-only", ""}, ¬
		{"file-like-not-link", "the readme.md has the setup steps", "text-only", ""}, ¬
		{"remember-note", "remember my gate code is 4521", "text-only", ""}, ¬
		{"remember-note-emoji", "remember the 🍜 pho place on 5th street is the best", "text-only", ""}, ¬
		{"recall-note", "what's my gate code?", "text-only", "4521"}, ¬
		{"recall-link", "what rust link did i save?", "text-only", "rust-lang.org"}, ¬
		{"save-image", "file:~/Pictures/satchel-test/sample-photo.jpg", "tapback", ""}, ¬
		{"save-video", "file:~/Pictures/satchel-test/sample-video.mp4", "tapback", ""}, ¬
		{"recall-image", "send me that picture", "attachment", ""}, ¬
		{"recall-video", "send me that video", "attachment", ""}, ¬
		{"save-voice", "file:~/Pictures/satchel-test/sample-voice.m4a", "tapback", ""}, ¬
		{"recall-voice", "send me that voice memo", "attachment", ""}, ¬
		{"question-not-saved", "send me the chili recipe", "text-only", ""}}
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
	set toSend to caseText
	if caseText starts with "file:~/" then
		set toSend to POSIX file ((POSIX path of (path to home folder)) & (text 8 thru -1 of caseText))
	end if
	tell application "Messages"
		set toSatchel to participant satchelNumber of (first account whose service type = iMessage)
		send toSend to toSatchel
	end tell
	set fromSatchel to "FROM message m JOIN handle h ON m.handle_id = h.ROWID WHERE h.id = '" & satchelNumber & "' AND m.is_from_me = 0 AND m.date > " & startDate
	set countQuery to "SELECT coalesce(sum(m.associated_message_type = " & likeTapback & "), 0) || '|' || " & ¬
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
