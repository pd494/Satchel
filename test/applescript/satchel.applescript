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
		{"search-today", "what did i save today?", "text", ""}, ¬
		{"search-list", "send me the stuff i saved yesterday", "text", ""}, ¬
		{"search-pick", "#2", "text", ""}, ¬
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
		{"s-find-missing-2", "where's that hotel booking i saved", "text-only", ""}, ¬
		{"s-list", "what have i saved so far", "text", ""}, ¬
		{"s-pick-after-list", "send me the sourdough one", "text", "wikipedia.org"}, ¬
		{"s-find-exact", "send me the tokio link", "text", "github.com"}, ¬
		{"s-find-typo", "that sourdoh thing", "text", "wikipedia.org"}, ¬
		{"s-save-bare", "https://en.wikipedia.org/wiki/Banana_bread", "tapback", ""}, ¬
		{"s-resend-recent", "send that back to me", "text", "Banana_bread"}, ¬
		{"s-find-many", "send me my bread links", "text", "2"}, ¬
		{"s-pick-number", "#1", "text", "wikipedia.org"}, ¬
		{"s-save-dup", "https://www.youtube.com/watch?v=4BOTvaRaDjI my glute warmup too", "tapback", ""}, ¬
		{"s-find-dup", "the glute warmup vid", "text", "youtube.com"}, ¬
		{"s-note-fix", "actually the cabin wifi is otter99 now", "text-only", ""}, ¬
		{"s-find-note-fix", "cabin wifi password?", "text-only", "otter99"}, ¬
		{"s-find-old-date", "what did i save last year?", "text-only", ""}, ¬
		{"c-save-burst", "https://www.tiktok.com/@scout2015/video/6718335390845095173||the corgi doing zoomies lol", "tapback", ""}, ¬
		{"c-save-inline", "https://www.instagram.com/reel/C0JKhWRx5zv/ thai green curry my aunt makes", "tapback", ""}, ¬
		{"c-chat", "ugh mondays", "text-only", ""}, ¬
		{"c-find-corgi", "send me the zoomies dog clip", "text", "tiktok.com"}, ¬
		{"c-find-curry", "that curry reel", "text", "instagram.com"}, ¬
		{"o-x", "https://x.com/elonmusk/status/1585341984679469056", "tapback", ""}, ¬
		{"o-instagram", "https://www.instagram.com/reel/C0JKhWRx5zv/", "tapback", ""}, ¬
		{"o-reddit", "https://www.reddit.com/r/rust/comments/1fh8gq5/", "tapback", ""}, ¬
		{"o-spotify", "https://open.spotify.com/track/4PTG3Z6ehGkBFwjybzWkR8", "tapback", ""}, ¬
		{"o-pinterest", "https://www.pinterest.com/pin/1196337398574536/", "tapback", ""}, ¬
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
		{"remind-cancel-none", "cancel my dentist reminder", "text-only", ""}, ¬
		{"zone-move", "i moved to nyc", "text-only", ""}, ¬
		{"zone-move-back", "actually i moved back to LA", "text-only", ""}, ¬
		{"m-reset", "reset", "text", "Satchel"}, ¬
		{"m-short-fact", "heyy. im veggie btw and my sister priya is in denver", "text", ""}, ¬
		{"m-short-recall", "wait where does she live", "text-only", "Denver"}, ¬
		{"m-save-link", "https://en.wikipedia.org/wiki/Alfama for april", "tapback", ""}, ¬
		{"m-save-note", "remember door code 4417 for the airbnb", "text-only", ""}, ¬
		{"m-no-save", "ugh so gray today", "text", ""}, ¬
		{"m-long-work", "ok so work today was honestly a mess and i need to vent for a sec. we had the quarterly planning thing that was supposed to take an hour and it took almost four, mostly because nobody had read the doc before the meeting and we spent the first ninety minutes just re-explaining what the roadmap even was. my manager kept saying we should be more async but then scheduled two more meetings to talk about being async which, sure. the part that really got me was when the design team showed up with a totally new version of the onboarding flow that none of us on eng had seen, and they wanted it shipped by the end of the month. i get that they worked hard on it but we literally just finished rebuilding the old one. anyway the other annoying thing is security rotated everyone's building access again so i had to go down to the front desk and get a new badge, and the new badge number is 88213, which i will absolutely forget by tomorrow. the lady at the desk was nice about it at least. after that i tried to actually get some coding done but slack would not stop pinging, mostly people asking where the planning doc was, which, it was linked in the invite. i ended up eating lunch at my desk at like 3pm which was just a sad granola bar and some cold coffee. on the bright side my coworker Dana covered for me in standup when i was stuck at the front desk, so i owe her a coffee. she is honestly the only reason that team works at all. i think tomorrow i am going to block my whole morning as focus time and just not answer anything until noon. we will see if that lasts. also the office ac was broken again so it was like 80 degrees in the meeting room which did not help anyones mood. by the end everyone was just agreeing to things to get out of there. i feel like half the decisions we made are going to get reversed next week when people actually think about them. whatever. at least its almost the weekend and i can stop thinking about roadmaps for two days. i really need to start looking at other teams or maybe other jobs, this is the third planning cycle in a row that went like this and i am kind of tired of it.", "text", ""}, ¬
		{"m-long-move", "so the move is finally kind of done but wow it was more than i expected. the movers showed up two hours late and then said the couch would not fit through the new door, which, it did, it just took three of us and a lot of swearing to tilt it the right way. the new apartment is nice though, way more light than the old place and the kitchen actually has counter space which i have never had before. the bad part is the closets are tiny so a bunch of stuff had to go into storage. i rented a unit in the building basement, its unit B214 on the second floor of the storage area, which is confusing because the basement has two floors apparently. i put the winter clothes, the camping gear, the old monitor, and like six boxes of books down there. i still have to figure out where the bike goes because the building does not allow bikes in the hallways and the bike room is full. the landlord said a spot might open up next month. internet guy comes on thursday so until then i am living off my phone hotspot which is painful. the neighbors seem nice, there is an older couple across the hall who already brought over cookies, which was very sweet. i also met a guy named Omar who lives upstairs and plays guitar at like 11pm but honestly it sounds good so i do not mind. i need to buy a shower curtain, a lamp for the bedroom, and some kind of shelf for the entryway because shoes are everywhere right now. my mom keeps calling to ask if i have unpacked yet and the answer is very much no. i think i will do the kitchen this weekend and leave the rest for whenever. the commute is better too, about fifteen minutes shorter each way, so that is a win. overall it was worth it but i am never moving again, or at least not for a few years. i also found out the laundry room only takes quarters which is so annoying in the year 2026, so now i have a jar of quarters on my counter like its the 90s.", "text", ""}, ¬
		{"m-implicit-wifi", "airbnb wifi is alfama2026 fyi", "text", ""}, ¬
		{"m-long-cook", "i tried cooking a real dinner tonight for the first time in the new place and it went surprisingly ok. i made a big pot of lentil soup with carrots, celery, onion, a ton of garlic, cumin, smoked paprika and a can of tomatoes, and then a lemon and some spinach at the end. i was going to put cashews on top for crunch like the recipe said but then i remembered i am allergic to cashews, not peanuts, peanuts are totally fine, it is only cashews and maybe pistachios a little, so i used toasted pumpkin seeds instead and honestly it was better. the bread from the bakery down the street was amazing, they do this seeded sourdough that is still warm at like 6pm. i want to get better at cooking because i spend way too much on takeout, like probably four hundred a month which is ridiculous. my plan is to cook two big things on sunday and eat them through the week. next week i want to try a chickpea curry and maybe a big tray of roasted vegetables with some kind of tahini sauce. i also need a better knife because the one i have is basically a butter knife at this point. my friend recommended some japanese one but it was like 150 dollars so maybe not yet. the stove in the new place is gas which i love, it heats up so much faster than the old electric one. the oven runs hot though, i burned the first batch of croutons in like four minutes. lesson learned. i also want to start meal prepping lunches so i stop eating granola bars at my desk at 3pm. if anyone at work sees me with actual tupperware they are going to be shocked. anyway the soup made like eight servings so i am set for a while. i froze half of it in those deli containers. i think cooking is going to be my winter project, along with actually unpacking the rest of the boxes.", "text", ""}, ¬
		{"m-long-trip", "ok lisbon planning update because i finally sat down and figured out the itinerary. we land on april 12 in the morning and the airbnb in alfama does not let us check in until 3, so the plan is to drop the bags at one of those luggage lockers near the cathedral and just wander and eat. day two is the big belem day, the tower, the monastery, and obviously the pasteis de belem place even though everyone says the line is long. day three is sintra, and the train to sintra leaves rossio station at 9:40, so we have to be out the door by 9 at the latest, which dev is going to hate because he is not a morning person at all. i want to do pena palace and the moorish castle and if we have energy quinta da regaleira with the well. day four is a lazy day, maybe the lx factory and the bookstore there, and a sunset at one of the miradouros. day five we were thinking about a day trip to cascais for the beach but it might still be cold in april so we will see. for food i have a whole list, a seafood place in cais do sodre that someone on reddit swore by, a tiny tasca near the airbnb, and the time out market which is touristy but fun. i also want to see a fado show one night but they are kind of expensive so maybe just one. we still need to figure out if we get the lisboa card or just pay as we go, the math is kind of close. dev wants to rent scooters but the hills there are no joke and i do not trust either of us on a scooter going downhill on cobblestones. i also need to buy a power adapter and maybe new walking shoes because my current ones are falling apart. flights are booked, airbnb is booked, sintra train tickets we can just buy at the station. honestly so excited, i have not taken a real vacation in like two years.", "text", ""}, ¬
		{"m-long-family", "family stuff this week was a lot but mostly good. my brother called to say they are doing a party for my nephew Theo, he turns 7 on march 9 and they are doing a dinosaur theme because he is obsessed, like he can name every dinosaur including ones i have never heard of. i need to figure out a gift, i was thinking a fossil dig kit or maybe one of those big dinosaur lego sets. my sister in law said he already has a lot of lego so maybe the fossil kit. my mom is flying in for the party which means i will probably have to host her for a couple nights at the new apartment, which means i actually do have to unpack before march, ugh. my dad is not coming because his knee surgery is the week before, he is fine but he cannot fly for a while after. i should call him more, i keep saying that. my grandma turned 91 last month and she is still sharper than all of us, she beat everyone at cards at thanksgiving and then went to bed at 8pm like a legend. my cousins are all spread out now, one is in seattle, one just moved to chicago for grad school, and the youngest is still in high school and wants to be a marine biologist. we have a group chat that is mostly memes and my aunt sending blurry photos of her garden. i also need to send my brother back the camping gear i borrowed last summer, which is now in my storage unit, so i need to remember to grab it. family is chaotic but i like that we are close. anyway i think the plan is to get Theo the fossil kit and maybe a dinosaur book to go with it, and to finally unpack the guest room so my mom has somewhere to sleep.", "text", ""}, ¬
		{"m-implicit-dentist", "ugh dentist moved my thing to dec 4 at 2", "text", ""}, ¬
		{"m-long-gym", "started going back to the gym this month and it is going better than i thought. i signed up for one of those packages with a personal trainer because i know myself and i will not go if nobody is expecting me. my trainer is called Bruno and he is very intense but also really nice, he used to play rugby in portugal which is funny given the trip. we do three sessions a week, monday wednesday friday at 7am, which is brutal but it means i get it done before work. mondays are legs, wednesdays are upper body, fridays are some kind of circuit thing that makes me want to lie on the floor forever. my goal is to deadlift my body weight by the summer, right now i am at about 70 percent of that. bruno says my form is decent but i keep rounding my back when i get tired, so we are working on that. i also bought real gym shoes finally instead of using my running shoes for everything. my lower back was sore the first week but it is much better now. i am also trying to walk more on the off days, like 8000 steps minimum. the gym is a ten minute walk from the new apartment which helps a lot, the old one was a drive. there is a sauna too which i have started using after friday sessions and it is the best part of the week honestly. bruno also gave me a meal plan but it has way too much chicken for a vegetarian so i am adapting it with tofu, lentils and greek yogurt. i want to keep this going through the trip, maybe just do bodyweight stuff in lisbon. three weeks in and i have not missed a session, which is a record for me.", "text", ""}, ¬
		{"m-long-show", "ok so i finally started that show everyone keeps telling me about, the one with the chef who goes back to run his family sandwich shop. i watched like five episodes in a row last night which was a mistake because i had the 7am gym session with bruno and i was a zombie. the show is so stressful though, every episode is basically people yelling in a tiny kitchen, and somehow that is relaxing to me? i think because it makes my work stress look tiny. the main actor is so good, you can feel how tired and anxious he is in every scene. there is one episode that is just one long take of a dinner service going wrong and i was holding my breath the whole time. my coworker Dana says season two is even better and that there is an episode with a whole family dinner that is chaos. i also started a book my friend lent me, a sci fi thing about a generation ship where nobody remembers why they left earth, it is slow but i am into it. i am trying to read more and scroll less, i put a screen time limit on my phone but i keep hitting ignore for 15 minutes which defeats the whole point. maybe i need to put my phone in another room at night. i also want to start a podcast list for the lisbon flight since it is like nine hours with the layover. if anyone has recs for history podcasts let me know, dev says he only listens to sports stuff which will not help me. anyway i am going to try to watch only two episodes tonight. we will see.", "text", ""}, ¬
		{"m-long-money", "trying to get my money stuff together before the trip because i realized i have no idea where it all goes. i finally made a spreadsheet with all my subscriptions and it was embarrassing, i am paying for four streaming services, two of which i forgot existed, a meditation app i opened once, and a cloud storage plan i do not need. i cancelled three of them and that is like 45 dollars a month back. rent at the new place is a bit higher but the commute savings and not driving to the gym kind of balance it out. my plan is to put 300 a month into a separate trip fund and then whatever is left over into savings. i also set up autopay for the credit card because i almost missed a payment last month during the move, which would have been dumb. for lisbon i think the whole trip will be around 2200 with flights, airbnb split with dev, food and the sintra stuff. i want to have an extra few hundred buffer for random things like a fado show or souvenirs for theo and my mom. i also need to tell my bank i am traveling so they do not freeze my card at the first pastry shop. my friend says to get a card with no foreign transaction fees, i should look into that this week. honestly the spreadsheet made me feel way more in control, i might actually keep updating it. or i will forget about it in two weeks like the meditation app, we will see.", "text", ""}, ¬
		{"f-gibberish", "asdkjh qwe 🫠🫠 ;;; DROP TABLE messages; -- lol", "text", ""}, ¬
		{"f-unicode", "café naïve 東京 مرحبا Привет 🇵🇹 ok", "text", ""}, ¬
		{"f-ramble", "random but i keep thinking about how i need a new winter coat, the one i have has a broken zipper and the lining is torn. also the gym gave me a locker finally, its 312, right by the sauna which is perfect. i saw a guy at the gym today doing curls in the squat rack which bruno says is a crime. the coffee place on my corner started doing oat milk for free which is huge. i keep meaning to call my dad about his knee. i also think i want to learn to make bread, like real bread not just buying the seeded sourdough. there is a class at the community center on saturdays. dev texted that he found cheaper flights but we already booked so whatever. my plant is dying, i think i overwatered it, it is a pothos which are supposed to be impossible to kill so that says a lot. i also need to renew my passport soon, i think it expires next year, i should check that before lisbon actually. anyway that is everything in my brain right now.", "text", ""}, ¬
		{"f-ramble2", "ok one more brain dump then i will stop. i finally booked the haircut for next tuesday, my usual barber left so i am trying someone new named Lou which is scary. the internet guy actually came on thursday and the wifi in the new place is fast which is great. i also ordered a lamp and a shelf for the entryway so the shoes situation is getting better. dana and i are thinking about doing a 10k in may, which is either a great idea or a terrible one. i bought a new pothos to replace the dead one, we will see if this one makes it. my mom wants to video call sunday at noon her time which is 2 hours ahead of me. i also need to figure out what to do with the old couch cushions that did not fit anything. and i want to try that chickpea curry tomorrow night if i have energy after the gym. ok that is all for real.", "text", ""}, ¬
		{"m-recall-storage", "whats the basement thing number", "text-only", "B214"}, ¬
		{"m-recall-train", "when do we gotta leave for the castle day", "text-only", "9:40"}, ¬
		{"m-recall-allergy", "what cant i eat again lol", "text-only", "cashew"}, ¬
		{"m-recall-sister", "wheres my sis at", "text-only", "Denver"}, ¬
		{"m-recall-wifi", "wifi for the place?", "text-only", "alfama2026"}, ¬
		{"m-recall-dentist", "when's the dentist again", "text-only", "4"}, ¬
		{"f-recall-locker", "which locker is mine", "text-only", "312"}, ¬
		{"r-raw-neighbor", "whats the upstairs guys name again", "text-only", "Omar"}, ¬
		{"r-raw-grandma", "how old did grandma turn", "text-only", "91"}, ¬
		{"m-recall-note", "the code for the place??", "text-only", "4417"}, ¬
		{"m-recall-link", "that neighborhood link", "text", "wikipedia.org"}, ¬
		{"m-saved-list", "what do u have of mine", "text", ""}, ¬
		{"m-correct", "oh p moved to austin btw", "text", ""}, ¬
		{"m-recall-correct", "so where is she now", "text-only", "Austin"}, ¬
		{"m-bs-fts", "find \"door\" AND NEAR(code* ^ OR )", "text", ""}, ¬
		{"m-bs-zone", "set my time zone to Mars/Olympus", "text", ""}, ¬
		{"m-bs-old-date", "what did we talk abt new years 2019", "text", ""}, ¬
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
