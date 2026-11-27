# Use restrictions for the character layering models

> **Pending legal review.** This document, and in particular the sentence under
> "Restrictions" that binds users, has not been reviewed by a lawyer.

This document applies to character layering in Myriad: the step that splits a
character portrait into layers (hair, face, clothes, …) and estimates their
depth with the See-through models, and every use of the layers and depth it
returns. The models and their licences are listed in
[THIRD_PARTY_MODELS.md](THIRD_PARTY_MODELS.md).

## Notice

The layering and depth models are subject to the use-based restrictions of the
CreativeML Open RAIL++-M License and the CreativeML Open RAIL-M License
(paragraph 5 and Attachment A of each).

## Restrictions

By using character layering you agree not to use it, the models behind it, or
their output for any of the uses listed in Attachment A below.

Attachment A, quoted verbatim from the CreativeML Open RAIL++-M License dated
July 26, 2023 ([`LICENSE.md` of Stable Diffusion XL base 1.0](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/blob/main/LICENSE.md),
[`LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt`](../../LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt)).
The Attachment A of the CreativeML Open RAIL-M License dated August 22, 2022
([`LICENSES/CreativeML-OpenRAIL-M.txt`](../../LICENSES/CreativeML-OpenRAIL-M.txt))
and of the CreativeML Open RAIL++-M License of Stable Diffusion 2, as reproduced
in the licence of the See-through Marigold Depth weights
([`LICENSES/LicenseRef-See-through-Marigold-weights.txt`](../../LICENSES/LicenseRef-See-through-Marigold-weights.txt)),
lists the same restrictions word for word.

> **Attachment A**
>
> **Use Restrictions**
>
> You agree not to use the Model or Derivatives of the Model:
>
> - In any way that violates any applicable national, federal, state, local or international law or regulation;
> - For the purpose of exploiting, harming or attempting to exploit or harm minors in any way;
> - To generate or disseminate verifiably false information and/or content with the purpose of harming others;
> - To generate or disseminate personal identifiable information that can be used to harm an individual;
> - To defame, disparage or otherwise harass others;
> - For fully automated decision making that adversely impacts an individual’s legal rights or otherwise creates or modifies a binding, enforceable obligation;
> - For any use intended to or which has the effect of discriminating against or harming individuals or groups based on online or offline social behavior or known or predicted personal or personality characteristics;
> - To exploit any of the vulnerabilities of a specific group of persons based on their age, social, physical or mental characteristics, in order to materially distort the behavior of a person pertaining to that group in a manner that causes or is likely to cause that person or another person physical or psychological harm;
> - For any use intended to or which has the effect of discriminating against individuals or groups based on legally protected characteristics or categories;
> - To provide medical advice and medical results interpretation;
> - To generate or disseminate information for the purpose to be used for administration of justice, law enforcement, immigration or asylum processes, such as predicting an individual will commit fraud/crime commitment (e.g. by text profiling, drawing causal relationships between assertions made in documents, indiscriminate and arbitrarily-targeted use).

## Where the models run

A Myriad site does not load these models. It sends the portrait to a
See-through Space on Hugging Face, and the models run there (see
[THIRD_PARTY_MODELS.md](THIRD_PARTY_MODELS.md)).
