#!/bin/bash

# Configuration
SERVER_URL="http://localhost:8080/v1/chat/completions"
MODEL="gemma-3-4b-it"

PROMPT=$(cat <<'PROMPT_EOF'
GOAL: 1. go to https://www.orange.ma
2. click search icon
3. type "roaming" (as is, do not translate) in search bar
4. click the first result
5. find out the price

AVAILABLE_CAPABILITIES:
navigate: chrome-devtools:navigate_page
newPage: chrome-devtools:new_page
snapshot: chrome-devtools:take_snapshot
click: chrome-devtools:click
fill: chrome-devtools:fill
typeText: chrome-devtools:type_text
pressKey: chrome-devtools:press_key
waitFor: chrome-devtools:wait_for
listPages: chrome-devtools:list_pages
selectPage: chrome-devtools:select_page
evaluate: chrome-devtools:evaluate_script
screenshot: chrome-devtools:take_screenshot

BROWSER_STATE:
uid=1_0 rootwebarea

PAGES:
## Pages
1: about:blank
2: about:blank [selected]

DOCUMENT:
{"title":"","url":"about:blank"}

{"action": "navigate", "url": "https://www.orange.ma", "reason": "Navigate to the target website as requested in the goal."}

STEP: 2
LAST_ACTION_RESULT: navigate -> OK: Called chrome-devtools:navigate_page

BROWSER_STATE:
BROWSER_STATE:
uid=2_0 rootwebarea "Orange Maroc - Orange"
  uid=2_1 banner
    uid=2_2 link "Particuliers"
      uid=2_3 statictext "Particuliers"
    uid=2_4 link "Entreprises"
      uid=2_5 statictext "Entreprises"
    uid=2_6 link "Pro"
      uid=2_7 statictext "Pro"
    uid=2_8 link "Corporate"
      uid=2_9 statictext "Corporate"
    uid=2_10 link "Wholesale"
      uid=2_11 statictext "Wholesale"
    uid=2_12 link "" description="Mon espace client"
      uid=2_13 statictext ""
    uid=2_14 link "" description="Boutique Orange"
      uid=2_13 statictext ""
    uid=2_15 link " 0"
      uid=2_13 statictext ""
      uid=2_16 statictext "0"
    uid=2_17 link "العربية"
      uid=2_18 statictext "العربية"
    uid=2_19 link "FR"
      uid=2_20 statictext "FR"
    uid=2_21 link "Orange Logo"
      uid=2_22 image "Orange Logo"
    uid=2_23 navigation
      uid=2_24 link "Recharges"
        uid=2_25 statictext "Recharges"
      uid=2_26 link "Forfaits"
        uid=2_27 statictext "Forfaits"
      uid=2_28 link "Yoxo.ma"
        uid=2_29 statictext "Yoxo.ma"
      uid=2_30 link "WiFi à la Maison"
        uid=2_31 statictext "WiFi à la Maison"
      uid=2_32 link "Divertissement"
        uid=2_33 statictext "Divertissement"
      uid=2_34 link "Orange Money"
        uid=2_35 statictext "Orange Money"
      uid=2_36 link "Assistance"
        uid=2_37 statictext "Assistance"
    uid=2_38 link ""
      uid=2_13 statictext ""
  uid=2_39 main
    uid=2_40 heading "Rejoindre Orange"
    uid=2_41 link "Acheter un Forfait à partir de 49 Dh"
      uid=2_42 statictext "Acheter un Forfait"
      uid=2_43 linebreak
      uid=2_44 statictext "à partir de 49 Dh"
    uid=2_45 link "Acheter le Wifi Orange Installation rapide"
      uid=2_46 statictext "Acheter le Wifi Orange"
      uid=2_47 linebreak
      uid=2_48 statictext "Installation rapide"
    uid=2_49 link "Acheter votre SIM Recharges"
      uid=2_50 statictext "Acheter votre SIM"
      uid=2_51 linebreak
      uid=2_52 statictext "Recharges"
    uid=2_53 link "Acheter un Smartphone à partir de 449 Dh"
      uid=2_54 statictext "Acheter un Smartphone"
      uid=2_55 linebreak
      uid=2_56 statictext "à partir de 449 Dh"
    uid=2_57 link
    uid=2_58 heading "La Fibre d'Orange"
    uid=2_59 statictext "Jusqu'à 1Gbps"
    uid=2_60 link "Découvrir"
      uid=2_61 statictext "Découvrir"
    uid=2_62 link
    uid=2_63 heading "*6 Kaaaayna !"
    uid=2_64 statictext "1Go offert via Orange Money"
    uid=2_65 link "Découvrir"
      uid=2_66 statictext "Découvrir"
    uid=2_67 link
    uid=2_68 heading "Galaxy S26 Series"
    uid=2_69 statictext "Crédit 100% gratuit à partir de 416 Dh/mois"
    uid=2_70 link "Découvrir"
      uid=2_71 statictext "Découvrir"
    uid=2_72 link
    uid=2_73 heading "Yo Max 5G"
    uid=2_74 statictext "Plus qu'un forfait"
    uid=2_75 link "Découvrir"
      uid=2_76 statictext "Découvrir"
    uid=2_77 link
    uid=2_78 heading "La Fibre d'Orange"
    uid=2_79 statictext "Jusqu'à 1Gbps"
    uid=2_80 link "Découvrir"
      uid=2_81 statictext "Découvrir"
    uid=2_82 link
    uid=2_83 heading "*6 Kaaaayna !"
    uid=2_84 statictext "1Go offert via Orange Money"
    uid=2_85 link "Découvrir"
      uid=2_86 statictext "Découvrir"
    uid=2_13 statictext ""
    uid=2_87 link "1"
      uid=2_88 statictext "1"
    uid=2_89 link "2"
      uid=2_90 statictext "2"
    uid=2_91 link "3"
      uid=2_92 statictext "3"
    uid=2_93 link "4"
      uid=2_94 statictext "4"
    uid=2_95 link ""
      uid=2_13 statictext ""
    uid=2_96 link "Découvrir"
      uid=2_97 statictext "Découvrir"
    uid=2_98 heading "Espace client"
    uid=2_13 statictext ""
    uid=2_99 statictext "Gérez tous vos services 24h/24 sur l'espace client"
    uid=2_100 link "Me connecter"
      uid=2_101 statictext "Me connecter"
    uid=2_102 link "Créez votre compte"
      uid=2_103 statictext "Créez votre compte"
    uid=2_104 heading "Douz L'forfait Yo Max 5G"
    uid=2_105 link
    uid=2_106 link
    uid=2_107 link
    uid=2_108 link
    uid=2_109 heading "Découvrez les offres Orange"
    uid=2_110 heading "Wifi à la maison"
    uid=2_111 statictext "Profitez du meilleur de l’expérience WiFi en illimité chez vous avec Orange"
    uid=2_112 link "La Fibre d’Orange Le WiFi multi usage à la vitesse de la lumière à partir de 249 Dh"
      uid=2_113 statictext "La Fibre d’Orange"
      uid=2_114 linebreak
      uid=2_115 statictext "Le WiFi multi usage à la vitesse de la lumière à partir de"
      uid=2_116 statictext "249 Dh"
    uid=2_117 link "L’ADSL Orange Le WiFi illimité accessible à partir de 149 Dh /mois"
      uid=2_118 statictext "L’ADSL Orange"
      uid=2_119 linebreak
      uid=2_120 statictext "Le WiFi illimité accessible à partir de"
      uid=2_121 statictext "149 Dh /mois"
      uid=2_122 linebreak
    uid=2_123 link "La DarBox 4G+ Le WiFi illimité sans installation qui vous accompagne partout"
      uid=2_124 statictext "La DarBox 4G+"
      uid=2_125 linebreak
      uid=2_126 statictext "Le WiFi illimité sans installation qui vous accompagne partout"
    uid=2_127 heading "Forfaits Mobiles"
    uid=2_128 statictext "Le meilleur du forfait, sans engagement, à partir de"
    uid=2_129 statictext "49 Dh/mois"
    uid=2_130 link "Forfaits Yo Les forfaits généreux, les avantages et le service en plus"
      uid=2_131 statictext "Forfaits Yo"
      uid=2_132 linebreak
      uid=2_133 statictext "Les forfaits généreux, les avantages et le service en plus"
    uid=2_134 link "Forfaits Yoxo Un max de générosité, exclusivement en ligne"
      uid=2_135 statictext "Forfaits Yoxo"
      uid=2_136 linebreak
      uid=2_137 statictext "Un max de générosité, exclusivement en ligne"
      uid=2_138 linebreak
    uid=2_139 link "Les Pass Personnalisez votre forfait à partir de 10 Dh"
      uid=2_140 statictext "Les Pass"
      uid=2_141 linebreak
      uid=2_142 statictext "Personnalisez votre forfait à partir de"
      uid=2_143 statictext "10 Dh"
    uid=2_144 heading "Divertissement"
    uid=2_145 statictext "Un univers de services divertissants et éducatifs s’offre à vous !"
    uid=2_146 link "Spotify Payez votre abonnement Spotify avec Orange à partir de 7 Dh seulement!"
      uid=2_147 statictext "Spotify"
      uid=2_148 linebreak
      uid=2_149 statictext "Payez votre abonnement Spotify avec Orange à partir de"
      uid=2_150 statictext "7 Dh"
      uid=2_151 statictext "seulement!"
    uid=2_152 link "Shahid Profitez du meilleurs des films et séries à partir de 19 Dh !"
      uid=2_153 statictext "Shahid"
      uid=2_154 linebreak
      uid=2_155 statictext "Profitez du meilleurs des films et séries à partir de"
      uid=2_156 statictext "19 Dh"
      uid=2_157 statictext "!"
    uid=2_158 link "Freefire Payez vos Diamants avec votre solde Orange à partir de 6 Dh !"
      uid=2_159 statictext "Freefire"
      uid=2_160 linebreak
      uid=2_161 statictext "Payez vos Diamants avec votre solde Orange à partir de"
      uid=2_162 statictext "6 Dh"
      uid=2_163 statictext "!"
    uid=2_164 link "Voir tous les produits"
      uid=2_165 statictext "Voir tous les produits"
    uid=2_166 heading "Smartphones et Accessoires"
    uid=2_167 link " Smartphones"
      uid=2_13 statictext ""
      uid=2_168 statictext "Smartphones"
    uid=2_169 link " Accessories"
      uid=2_13 statictext ""
      uid=2_170 statictext "Accessories"
    uid=2_171 link " Protection téléphone"
      uid=2_13 statictext ""
      uid=2_172 statictext "Protection téléphone"
    uid=2_173 link " Crédit gratuit"
      uid=2_13 statictext ""
      uid=2_174 statictext "Crédit gratuit"
    uid=2_175 heading "Max it"
    uid=2_176 statictext "Consulter mon solde"
    uid=2_177 statictext "Recharger ma ligne"
    uid=2_178 statictext "Payer mon forfait"
    uid=2_179 button " Télécharger"
    uid=2_180 link "Toute l'assistance"
      uid=2_181 statictext "Toute l'assistance"
    uid=2_182 heading "Urgence et dépannage"
    uid=2_183 link " Mobile volé"
      uid=2_13 statictext ""
      uid=2_184 statictext "Mobile volé"
    uid=2_185 link " Configuration internet"
      uid=2_13 statictext ""
      uid=2_186 statictext "Configuration internet"
    uid=2_187 link " Code PUK"
      uid=2_13 statictext ""
      uid=2_188 statictext "Code PUK"
    uid=2_189 link " Activer un forfait"
      uid=2_13 statictext ""
      uid=2_190 statictext "Activer un forfait"
    uid=2_191 link " Recharger"
      uid=2_13 statictext ""
      uid=2_192 statictext "Recharger"
    uid=2_193 link " Payer ma facture"
      uid=2_13 statictext ""
      uid=2_194 statictext "Payer ma facture"
    uid=2_195 link " Roaming"
      uid=2_13 statictext ""
      uid=2_196 statictext "Roaming"
  uid=2_197 contentinfo
    uid=2_198 statictext "Suivez-nous"
    uid=2_199 link "" description="WhatsApp"
      uid=2_13 statictext ""
    uid=2_200 link "" description="Facebook"
      uid=2_13 statictext ""
    uid=2_201 link "" description="Twitter"
      uid=2_13 statictext ""
    uid=2_202 link "" description="Instagram"
      uid=2_13 statictext ""
    uid=2_203 link "" description="Youtube"
      uid=2_13 statictext ""
    uid=2_204 link "" description="LinkedIn"
      uid=2_13 statictext ""
    uid=2_205 heading "Liens utiles"
    uid=2_206 link "Acheter une recharge ou un pass"
      uid=2_207 statictext "Acheter une recharge ou un pass"
    uid=2_208 link "Payer vos factures"
      uid=2_209 statictext "Payer vos factures"
    uid=2_210 link "Configurer votre mobile"
      uid=2_211 statictext "Configurer votre mobile"
    uid=2_212 link "Estimer votre consommation internet"
      uid=2_213 statictext "Estimer votre consommation internet"
    uid=2_214 link "Trouver une boutique"
      uid=2_215 statictext "Trouver une boutique"
    uid=2_216 heading "Boutique en ligne"
    uid=2_217 link "Xiaomi"
      uid=2_218 statictext "Xiaomi"
    uid=2_219 link "Apple"
      uid=2_220 statictext "Apple"
    uid=2_221 link "Samsung"
      uid=2_222 statictext "Samsung"
    uid=2_223 link "Huawei"
      uid=2_224 statictext "Huawei"
    uid=2_225 link "STG"
      uid=2_226 statictext "STG"
    uid=2_227 link "Assistance"
      uid=2_228 statictext "Assistance"
    uid=2_229 link "Forfait Orange"
      uid=2_230 statictext "Forfait Orange"
    uid=2_231 link "Recharge Orange"
      uid=2_232 statictext "Recharge Orange"
    uid=2_233 link "Récupérer mon numéro"
      uid=2_234 statictext "Récupérer mon numéro"
    uid=2_235 link "Questions fréquentes"
      uid=2_236 statictext "Questions fréquentes"
    uid=2_237 link "Urgences et dépannage"
      uid=2_238 statictext "Urgences et dépannage"
    uid=2_239 link "Espace client"
      uid=2_240 statictext "Espace client"
    uid=2_241 link "Equipements"
      uid=2_242 statictext "Equipements"
    uid=2_243 link "Factures et paiements"
      uid=2_244 statictext "Factures et paiements"
    uid=2_245 link "Espace client"
      uid=2_246 statictext "Espace client"
    uid=2_247 link "Espace client"
      uid=2_248 statictext "Espace client"
    uid=2_249 link "Max it"
      uid=2_250 statictext "Max it"
    uid=2_251 link "Cadeau du vendredi"
      uid=2_252 statictext "Cadeau du vendredi"
    uid=2_253 link "Ma ligne"
      uid=2_254 statictext "Ma ligne"
    uid=2_255 link "Orange Cinéday"
      uid=2_256 statictext "Orange Cinéday"
    uid=2_257 link "Contactez-nous"
      uid=2_258 statictext "Contactez-nous"
    uid=2_259 link "Politique de protection des données"
      uid=2_260 statictext "Politique de protection des données"
    uid=2_261 link "Conditions générales d'utilisation"
      uid=2_262 statictext "Conditions générales d'utilisation"
    uid=2_263 link "Catalogue tarifaire"
      uid=2_264 statictext "Catalogue tarifaire"
    uid=2_265 link "Plan du site"
      uid=2_266 statictext "Plan du site"
    uid=2_267 statictext "© 2026 Orange"
  uid=2_268 button "Djingo, le chatbot d’Orange. Une question ? Demandez-moi !"

PAGES:
## Pages
1: about:blank
2: https://www.orange.ma/ [selected]

DOCUMENT:
Script ran on page and returned:
{"title":"Orange Maroc - Orange","url":"https://www.orange.ma/"}
PROMPT_EOF
)

echo "Testing Chat Completion (JSON Mode)"
echo "Target: $SERVER_URL"
echo "Model: $MODEL"
echo "-------------------------------------"

# Use jq to build the JSON payload safely (handles newlines and special chars)
PAYLOAD=$(jq -n \
  --arg model "$MODEL" \
  --arg prompt "$PROMPT" \
  '{
    model: $model,
    messages: [{role: "user", content: $prompt}],
    stream: false
  }')

curl -s -X POST "$SERVER_URL" \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" | jq .
