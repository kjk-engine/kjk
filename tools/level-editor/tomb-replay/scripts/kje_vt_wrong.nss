// A wrong answer at the terminal: the tomb sets two of its beasts on the party.
void main()
{
    object oTerm = GetObjectByTag("kje_vt_term");
    location lHere = GetLocation(oTerm);
    object oOne = CreateObject(OBJECT_TYPE_CREATURE, "g_tukata02", lHere);
    object oTwo = CreateObject(OBJECT_TYPE_CREATURE, "g_tukata02", lHere);
    SendMessageToPC(GetFirstPC(), "The tomb answers a wrong word with its beasts.");
}
