## Redelete removes all of your reddit comments and submissions. 

### Quickstart
```
// authorize your reddit account with this app:
$ redelete authorize

// add configuration options to the username you just authorized
// add subreddit exclusions (space separated list of subreddits)
$ redelete config <username> -a webdev reactjs rust

// add a minimum score to avoid deleting posts higher than this score
$ redelete config <username> -s 500

// add a max time to avoid deleting posts made newer than this time (in hours)
$ redelete config <username> -t 5

// do them all at once
$ redelete config <username> -a webdev reactjs rust -s 500 -t 5

// dry-run the app
$ redelete run -d <username>

// run the app and actually delete your posts
$ redelete run <username>

// view your config options for any given username
$ redelete view <username>

// help
$ redelete -h
$ redelete run -h
$ redelete config -h
$ redelete view -h

```

### You can configure the application to skip
* posts in specific subreddits
* posts newer than certain amount of hours
* posts above a certain minimum score

This is my first rust app, so all feedback is welcome (negative or positive).

#### Still needs
* Edit-before-deletion logic. Still need to verify whether this makes a difference or not on reddit's servers.
* General code cleanup: reorganization, remove comments, unused imports, dead code, etc.
* Improved output (formatting, colors, etc)
* A better readme

#### Caveats
* Pushshift and other similar services will still index your posts
* Once your account is authorized, the refresh oauth token is stored in a plain text file under your user account. The only thing the token can do is let someone use the reddit api to:
  * read your posts/comments/upvotes/downvotes and other history info
  * read your account preferences and trophies
  * edit/delete your posts. 
* The app makes no efforts whatsoever to secure this token beyond the OS's file basic security/permissions.
* To further secure the conf file, I would `chown -R <YOUR_USERNAME>:<ANY_GROUP> ~/.config/redelete` and `chmod -R 700 ~/.config/redelete` once you've authorized any reddit accounts. If someone gets root access or access to your login you're screwed, though I imagine you'd have much more to lose than your reddit account in this scenario.
* For Windows, I *think* the conf file is naturally secured as it's in your AppData folder, but I could be wrong there. 