# Tabsbot

Tabsbot is a small matrix bot that can be used in a room to keep track of money exchanges in a room.

An example use-case would for flatmates willing to keep track of who paid how much to fill the fridge, to balance out over time.

## Use it on matrix

Once the bot is in the room, it recognises 4 commands:

```
!balance
```

This displays the current value of the tabs in this room for each known user. The displayed value
represents **how much the group owes to this person**.

As such, a positive amount means this person has given to the group more than it has received, while
a negative amount means this person is late and should be the next one to pay, for example.

```
!paid <amount> [for some reason]
```

This command means you have paid `<amount>` for the group. This amount will be added to your tab.

Anything you write after the amount will just be repeated by the bot as an acknowledgement. The bot
does not use this value, but it can serve in the room to keep track of why a person paid.

```
!paidto <username> <amount> [for some reason]
```

This commands register that you have paid some amount to a particular member. This amount will be
added to your tab and substracted from their.

This can be used if someone lended some money to an other member for example, or for a member to compensate
their debt by directly paying an other member rather than buying something for the group.

```
!rebalance
```

Compute the means of all tabs and substract it from each tab, so that the global sum of the tabs is zero.

After many money transfert, all tabs will typically be large positive numbers, while the real information of
interest is the difference between them. This command can be used to get a better view of who is behind.

## Permissions

The bot stores the tab for the room in a state key in the matrix room, for improved robustness. As such, it requires
the `m.add_state_level` permission. Riot does not seem to give direct access to this permission level, so this will
typicall require to make the bot moderator (power level 50).

## Where does it run?

I host an instance of this bot as `@tabsbot:safaradeg.net`, but you can easily run you own as well: simply compile
the project, and run the program with given arguments:

```
matrix-tabs <SERVER> <USERNAME> <NAMESPACE>
```

Here, `<SERVER>` is the URL of the homeserver that hosts the account of the bot, `<USERNAME>` is the username
of this account, and `<NAMESPACE>` is a name-spacing value for the state key the bot will use (for example
my isntance of the bot uses `net.safaradeg.tabsbot`.

The bot will then prompt you for the password of the account, and then run.
