// A login script for BadTerm: put it in the scripts folder
// (badterm --paths shows where), then name it as an entry's Script in
// the dialing directory, or run it with Alt-J during a call.
//
// It answers the usual login questions with the entry's user name and
// password, says no to the ANSI-detection or "new user" questions some
// boards ask, and stops at the main menu.

// Some boards ask to press a key, or whether the screen shows ANSI.
let first = wait_any(["Press any key", "ANSI", "Login:", "name:", "NAME:"], 30);
if first == 0 {
    send("\r");
} else if first == 1 {
    send("Y");
}

if first < 2 && wait_any(["Login:", "name:", "NAME:"], 30) < 0 {
    throw "no login question came";
}
send(user() + "\r");

if !wait("assword", 20) {
    throw "no password question came";
}
send(password() + "\r");

// Past any "read new mail?" or "scan messages?" questions to the menu.
loop {
    let next = wait_any(["[Y/n]", "[y/N]", "(Y/N)", "Main Menu", "MAIN MENU", "Command:"], 30);
    if next < 0 || next >= 3 {
        break;
    }
    send("n");
}
notify("Logged in to " + bbs());
