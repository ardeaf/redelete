### Redelete removes all of your reddit comments and submissions. 

#### You can configure the application to skip:
* posts in specific subreddits
* posts newer than certain amount of hours
* posts above a certain minimum score

It is 80% there. Most of the logic is done.

All pull requests, suggestions, feedback is welcome.

#### Still needs:
* Rate limiting logic, since reddit rate limits at 60 req/min
* Currently always dry runs, since the rate limiting hasn't been implemented yet
* Edit-before-deletion logic. Still need to verify whether this makes a difference or not on reddit's servers.
* A better readme

#### Caveats:
* Pushshift and other similar services will still index your posts
* Once your account is authorized, the refresh oauth token is stored in a plain text file under your user account. The only thing the token can do is let someone use the reddit api to:
  * read your posts/comments/upvotes/downvotes and other history info
  * read your account preferences and trophies
  * edit/delete your posts. 
* The app makes no efforts whatsoever to secure this token beyond the OS's file basic security/permissions.
* To further secure the conf file, I would `chown -R <YOUR_USERNAME>:<ANY_GROUP> ~/.config/redelete` and `chmod -R 700 ~/.config/redelete` once you've authorized any reddit accounts. If someone gets root access or access to your login you're screwed, though I imagine you'd have much more to lose than your reddit account in this scenario.
* For Windows, I *think* the conf file is naturally secured as it's in your AppData folder, but I could be wrong there. 