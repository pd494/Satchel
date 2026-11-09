property contactName : "Real Satchel"
property timeoutSeconds : 90
property chatDb : "~/Library/Messages/chat.db"
property likeTapback : 2001

on run
	tell application "Contacts" to set satchelNumber to value of first phone of (first person whose name is contactName)
	set satchelNumber to do shell script "echo " & quoted form of satchelNumber & " | tr -d ' ()-'"

	set results to {}
	set failures to 0
	set cases to {¬
		{"greeting", "hi", "text", ""}, ¬
		{"save link", "https://www.rust-lang.org/learn", "tapback", ""}, ¬
		{"save note", "remember my gate code is 4521", "tapback", ""}, ¬
		{"recall note", "what's my gate code?", "text", "4521"}, ¬
		{"recall link", "what rust link did i save?", "text", "rust-lang.org"}}

	repeat with c in cases
		set {caseName, outgoing, expectKind, expectText} to c
		if my runCase(satchelNumber, outgoing, expectKind, expectText) then
			set end of results to "PASS  " & caseName
		else
			set failures to failures + 1
			set end of results to "FAIL  " & caseName & " (no matching reply in " & timeoutSeconds & "s)"
		end if
	end repeat

	set AppleScript's text item delimiters to linefeed
	set report to results as text
	if failures > 0 then error report number failures
	return report
end run

on runCase(satchelNumber, outgoing, expectKind, expectText)
	set startDate to do shell script "echo $(( ($(date +%s) - 978307200) * 1000000000 ))"
	tell application "Messages"
		set toSatchel to participant satchelNumber of (first account whose service type = iMessage)
		send outgoing to toSatchel
	end tell
	set query to "SELECT count(*) FROM message m JOIN handle h ON m.handle_id = h.ROWID" & ¬
		" WHERE h.id = '" & satchelNumber & "' AND m.is_from_me = 0 AND m.date > " & startDate & ¬
		" AND " & my matchClause(expectKind, expectText)
	repeat (timeoutSeconds div 3) times
		delay 3
		set found to do shell script "sqlite3 -readonly " & chatDb & " " & quoted form of query
		if (found as integer) > 0 then return true
	end repeat
	return false
end runCase

on matchClause(expectKind, expectText)
	if expectKind is "tapback" then return "m.associated_message_type = " & likeTapback
	set clause to "m.associated_message_type = 0"
	if expectText is "" then return clause
	return clause & " AND (instr(lower(coalesce(m.text, '')), '" & expectText & "') > 0" & ¬
		" OR instr(hex(m.attributedBody), hex('" & expectText & "')) > 0)"
end matchClause
